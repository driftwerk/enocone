//! Publishing runs and showing them: the publish API, publish tokens, the
//! project page, run pages and the retention sweep.
//!
//! A publisher sends `results.json` and PNG screenshots, never HTML. The site
//! renders every page itself with the same renderer as local reports.

use std::collections::BTreeMap;
use std::path::{Path as FsPath, PathBuf};

use axum::Json;
use axum::extract::{Form, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use base64::Engine as _;
use enocone_spec::results::{RESULTS_SCHEMA, RunResults, Status};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::db::{self, Project, Role, User};
use crate::pages::{self, esc};
use crate::{App, Fail, R, auth, project_for, require_user, same_origin, visible_projects};

/// The largest publish accepted: results plus screenshots, base64-encoded.
pub const MAX_UPLOAD: usize = 80 * 1024 * 1024;
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
/// The most screenshot data one run may carry.
pub const MAX_SCREENSHOTS: usize = 50 * 1024 * 1024;

fn run_dir(app: &App, project_id: i64, run: i64) -> PathBuf {
    app.cfg.data.join("runs").join(project_id.to_string()).join(run.to_string())
}

// ------------------------------------------------------------------ API auth

/// The person behind a publish token (`Authorization: Bearer enc_…`).
fn token_user(app: &App, h: &HeaderMap) -> anyhow::Result<Option<User>> {
    let Some(tok) = auth::bearer(h) else { return Ok(None) };
    app.db.with(|c| {
        let uid: Option<i64> = c.query_row("SELECT user_id FROM tokens WHERE token_hash = ?1", [auth::hash(&tok)], |r| r.get(0)).optional()?;
        let Some(uid) = uid else { return Ok(None) };
        c.execute("UPDATE tokens SET last_used_at = ?1 WHERE token_hash = ?2", params![db::now(), auth::hash(&tok)])?;
        db::user_by_id(c, uid)
    })
}

fn api_error(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

pub async fn whoami(State(app): State<App>, h: HeaderMap) -> R {
    let Some(user) = token_user(&app, &h)? else { return Ok(api_error(StatusCode::UNAUTHORIZED, "Sign in with `enocone login`, or set ENOCONE_HUB_TOKEN.")) };
    let projects: Vec<String> = visible_projects(&app, &user)?.into_iter().map(|p| p.slug).collect();
    Ok(Json(json!({ "name": user.label(), "email": user.email, "github": user.github_login, "projects": projects })).into_response())
}

#[derive(Deserialize)]
pub struct PublishBody {
    results: Value,
    /// Screenshots by the path `results.json` uses (e.g. `assets/<flow>/1-3.png`), base64.
    #[serde(default)]
    files: BTreeMap<String, String>,
    #[serde(default)]
    source: Option<String>,
}

/// A path the results may use for a screenshot: relative, plain characters, `.png`.
pub fn safe_asset_path(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 300
        && p.ends_with(".png")
        && !p.starts_with('/')
        && p.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != ".." && !seg.starts_with('.'))
        && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b))
}

fn looks_like_jwt(t: &str) -> bool {
    !t.starts_with("enc_") && t.split('.').count() == 3
}

/// A run from GitHub Actions, vouched for by GitHub's OIDC token.
async fn publish_from_ci(app: &App, jwt: &str, slug: &str, body: Result<Json<PublishBody>, axum::extract::rejection::JsonRejection>) -> R {
    let claims = match app.oidc.verify(jwt, &app.cfg.origin()).await {
        Ok(c) => c,
        Err(why) => return Ok(api_error(StatusCode::UNAUTHORIZED, &format!("GitHub's token wasn't accepted: {why}"))),
    };
    if claims.event_name == "pull_request_target" {
        return Ok(api_error(StatusCode::FORBIDDEN, "Workflows triggered by pull_request_target can't publish: they may be running a fork's code."));
    }
    let repo = claims.repository.to_lowercase();
    let project = app.db.with(|c| db::project_by_slug(c, slug))?.filter(|p| p.repo.as_deref() == Some(repo.as_str()));
    let Some(project) = project else {
        return Ok(api_error(StatusCode::FORBIDDEN, &format!("A workflow in {repo} can't publish to {slug}: the project isn't linked to that repository.")));
    };
    let Json(body) = match body {
        Ok(b) => b,
        Err(e) => return Ok(api_error(StatusCode::BAD_REQUEST, &format!("The upload isn't valid JSON: {e}"))),
    };
    let by = if claims.actor.is_empty() { "GitHub Actions".to_string() } else { format!("{} via GitHub Actions", claims.actor) };
    store_run(app, &project, &by, "ci", body.results, body.files)
}

/// Runs of a project, newest first (for tokens of its members).
pub async fn list_runs(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>) -> R {
    let Some(user) = token_user(&app, &h)? else { return Ok(api_error(StatusCode::UNAUTHORIZED, "Sign in with `enocone login`, or set ENOCONE_HUB_TOKEN.")) };
    let Ok((project, _)) = project_for(&app, &user, &slug) else { return Ok(api_error(StatusCode::NOT_FOUND, "No such project, or you are not a member of it.")) };
    let runs: Vec<Value> = runs_of(&app, &project, 100)?
        .into_iter()
        .map(|r| json!({ "id": r.id, "url": format!("{}/{}/runs/{}/", app.cfg.origin(), project.slug, r.id), "source": r.source, "published_by": r.by, "branch": r.branch, "executed": r.executed, "prototype": r.prototype, "summary": r.summary, "created_at": r.created_at }))
        .collect();
    Ok(Json(json!({ "runs": runs })).into_response())
}

pub async fn publish(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>, body: Result<Json<PublishBody>, axum::extract::rejection::JsonRejection>) -> R {
    if let Some(t) = auth::bearer(&h).filter(|t| looks_like_jwt(t)) {
        return publish_from_ci(&app, &t, &slug, body).await;
    }
    let Some(user) = token_user(&app, &h)? else { return Ok(api_error(StatusCode::UNAUTHORIZED, "Sign in with `enocone login`, or set ENOCONE_HUB_TOKEN.")) };
    let project = match project_for(&app, &user, &slug) {
        Ok((p, r)) if r >= Role::Member => p,
        Ok(_) => return Ok(api_error(StatusCode::FORBIDDEN, "Reviewers can't publish runs; ask a project owner to make you a member.")),
        Err(_) => return Ok(api_error(StatusCode::NOT_FOUND, "No such project, or you are not a member of it.")),
    };
    let Json(body) = match body {
        Ok(b) => b,
        Err(e) => return Ok(api_error(StatusCode::BAD_REQUEST, &format!("The upload isn't valid JSON: {e}"))),
    };
    let source = match body.source.as_deref() {
        Some("ci") => "ci",
        _ => "cli",
    };
    store_run(&app, &project, &user.label(), source, body.results, body.files)
}

/// Validate and store a run; shared by token and CI publishing.
pub(crate) fn store_run(app: &App, project: &Project, by: &str, source: &str, results: Value, files: BTreeMap<String, String>) -> R {
    if results.get("schema").and_then(Value::as_str) != Some(RESULTS_SCHEMA) {
        return Ok(api_error(StatusCode::UNPROCESSABLE_ENTITY, &format!("results.json must have \"schema\": \"{RESULTS_SCHEMA}\"; update enocone and run again.")));
    }
    let parsed: RunResults = match serde_json::from_value(results) {
        Ok(r) => r,
        Err(e) => return Ok(api_error(StatusCode::UNPROCESSABLE_ENTITY, &format!("results.json doesn't match {RESULTS_SCHEMA}: {e}"))),
    };
    let mut pngs: Vec<(String, Vec<u8>)> = Vec::new();
    for (path, b64) in files {
        if !safe_asset_path(&path) {
            return Ok(api_error(StatusCode::UNPROCESSABLE_ENTITY, &format!("{path}: only screenshots (.png, under the run folder) can be published.")));
        }
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64.as_bytes()) else {
            return Ok(api_error(StatusCode::UNPROCESSABLE_ENTITY, &format!("{path}: not valid base64.")));
        };
        if !bytes.starts_with(PNG_MAGIC) {
            return Ok(api_error(StatusCode::UNPROCESSABLE_ENTITY, &format!("{path}: not a PNG image.")));
        }
        pngs.push((path, bytes));
    }
    if pngs.iter().map(|(_, b)| b.len()).sum::<usize>() > MAX_SCREENSHOTS {
        return Ok(api_error(StatusCode::PAYLOAD_TOO_LARGE, "A run may carry at most 50 MB of screenshots; take fewer (screenshots = \"on-failure\")."));
    }
    // Screenshots the results point at must be among the files, or are dropped.
    let mut data = parsed;
    let have: std::collections::HashSet<&str> = pngs.iter().map(|(p, _)| p.as_str()).collect();
    for fr in &mut data.flows {
        for a in &mut fr.attempts {
            for s in &mut a.steps {
                if s.screenshot.as_deref().is_some_and(|p| !have.contains(p)) {
                    s.screenshot = None;
                }
            }
        }
    }
    let flows: Vec<Value> = data
        .flows
        .iter()
        .map(|f| {
            let steps: Vec<&str> = f.attempts.last().map(|a| a.steps.iter().map(|s| status_class(s.status)).collect()).unwrap_or_default();
            json!({ "id": f.flow.id, "title": f.flow.title, "area": f.flow.meta.area, "stage": stage_str(f), "status": status_class(f.status), "steps": steps, "error": first_error(f) })
        })
        .collect();
    let summary = serde_json::to_string(&data.summary)?;
    let run_id = app.db.with(|c| {
        c.execute(
            "INSERT INTO runs (project_id, run_id, executed, prototype, source, published_by, branch, commit_sha, summary, flows, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![project.id, data.run.id, data.run.executed, data.run.prototype, source, by, data.run.git_branch, data.run.git_commit, summary, serde_json::to_string(&flows).unwrap(), db::epoch()],
        )?;
        let run_id = c.last_insert_rowid();
        for f in &data.flows {
            let doc = serde_json::to_string(&f.flow).unwrap_or_default();
            c.execute(
                "INSERT INTO flows (project_id, flow_id, title, area, stage, doc, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT (project_id, flow_id) DO UPDATE SET title = excluded.title, area = excluded.area, stage = excluded.stage, doc = excluded.doc, updated_at = excluded.updated_at",
                params![project.id, f.flow.id, f.flow.title, f.flow.meta.area, stage_str(f), doc, db::epoch()],
            )?;
        }
        if data.theme.is_some() {
            c.execute("UPDATE projects SET theme = ?1 WHERE id = ?2", params![data.theme, project.id])?;
        }
        Ok(run_id)
    })?;
    let dir = run_dir(app, project.id, run_id);
    let written = (|| -> anyhow::Result<()> {
        std::fs::create_dir_all(&dir)?;
        for (path, bytes) in &pngs {
            let target = dir.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(target, bytes)?;
        }
        std::fs::write(dir.join("results.json"), serde_json::to_vec(&data)?)?;
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(&dir);
        app.db.with(|c| c.execute("DELETE FROM runs WHERE id = ?1", [run_id]))?;
        return Err(e.into());
    }
    let url = format!("{}/{}/runs/{run_id}/", app.cfg.origin(), project.slug);
    Ok((StatusCode::CREATED, Json(json!({ "id": run_id, "url": url, "screenshots": pngs.len() }))).into_response())
}

fn status_class(s: Status) -> &'static str {
    match s {
        Status::Passed => "passed",
        Status::Failed => "failed",
        Status::Flaky => "flaky",
        Status::Skipped => "skipped",
        Status::Pending => "pending",
        Status::NotRun => "not-run",
    }
}

fn stage_str(f: &enocone_spec::results::FlowResult) -> String {
    serde_json::to_value(f.flow.meta.stage).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

fn first_error(f: &enocone_spec::results::FlowResult) -> Option<String> {
    let a = f.attempts.last()?;
    let s = a.steps.iter().find(|s| s.status == Status::Failed)?;
    Some(format!("Step {}: {}", s.key, s.error.as_ref().map(|e| e.message.as_str()).unwrap_or("failed")))
}

// ------------------------------------------------------------------ retention

/// Delete runs older than the retention period. Flow documents live in git and comments are kept.
pub fn sweep(app: &App, now: i64) -> anyhow::Result<usize> {
    let cutoff = now - i64::from(app.cfg.run_retention_days) * 86400;
    let old: Vec<(i64, i64)> = app.db.with(|c| {
        let mut st = c.prepare("SELECT id, project_id FROM runs WHERE created_at < ?1")?;
        st.query_map([cutoff], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
    })?;
    for (id, pid) in &old {
        let _ = std::fs::remove_dir_all(run_dir(app, *pid, *id));
        app.db.with(|c| c.execute("DELETE FROM runs WHERE id = ?1", [id]))?;
    }
    Ok(old.len())
}

// ------------------------------------------------------------------ pages

struct RunRow {
    id: i64,
    executed: bool,
    prototype: bool,
    source: String,
    by: String,
    branch: Option<String>,
    summary: Value,
    flows: Vec<Value>,
    created_at: i64,
}

fn runs_of(app: &App, project: &Project, limit: i64) -> anyhow::Result<Vec<RunRow>> {
    app.db.with(|c| {
        let mut st =
            c.prepare("SELECT id, executed, prototype, source, published_by, branch, summary, flows, created_at FROM runs WHERE project_id = ?1 ORDER BY created_at DESC, id DESC LIMIT ?2")?;
        st.query_map(params![project.id, limit], |r| {
            Ok(RunRow {
                id: r.get(0)?,
                executed: r.get(1)?,
                prototype: r.get(2)?,
                source: r.get(3)?,
                by: r.get(4)?,
                branch: r.get(5)?,
                summary: serde_json::from_str(&r.get::<_, String>(6)?).unwrap_or(Value::Null),
                flows: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
                created_at: r.get(8)?,
            })
        })?
        .collect()
    })
}

pub fn open_comment_count(app: &App, project: &Project) -> anyhow::Result<usize> {
    app.db.with(|c| c.query_row("SELECT COUNT(*) FROM comments WHERE project_id = ?1 AND resolved_at IS NULL AND parent_id IS NULL", [project.id], |r| r.get::<_, i64>(0))).map(|n| n as usize)
}

fn ago(epoch: i64) -> String {
    let d = (db::epoch() - epoch).max(0);
    match d {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", d / 60),
        3600..86400 => format!("{} h ago", d / 3600),
        _ => format!("{} days ago", d / 86400),
    }
}

fn nav<'a>(app: &'a App, site: &'a str, user: &'a User, projects: &'a [Project], project: &'a Project, section: &'a str) -> anyhow::Result<pages::Nav<'a>> {
    Ok(pages::Nav { site, user, projects, current: Some(project), section, review_count: open_comment_count(app, project)? })
}

struct FlowRow {
    id: String,
    title: String,
    area: Option<String>,
    stage: String,
}

fn flows_of(app: &App, project: &Project) -> anyhow::Result<Vec<FlowRow>> {
    app.db.with(|c| {
        let mut st = c.prepare("SELECT flow_id, title, area, stage FROM flows WHERE project_id = ?1 ORDER BY COALESCE(area, ''), title")?;
        st.query_map([project.id], |r| Ok(FlowRow { id: r.get(0)?, title: r.get(1)?, area: r.get(2)?, stage: r.get(3)? }))?.collect()
    })
}

fn open_comments_on(app: &App, project: &Project, flow: &str) -> anyhow::Result<i64> {
    app.db.with(|c| c.query_row("SELECT COUNT(*) FROM comments WHERE project_id = ?1 AND flow_id = ?2 AND resolved_at IS NULL AND parent_id IS NULL", params![project.id, flow], |r| r.get(0)))
}

pub fn project_page(app: &App, user: &User, project: &Project) -> R {
    let runs = runs_of(app, project, 20)?;
    let flows = flows_of(app, project)?;
    let latest = runs.first();
    let n = |k: &str| latest.and_then(|r| r.summary.get(k)).and_then(Value::as_u64).unwrap_or(0);
    let reviewing = flows.iter().filter(|f| matches!(f.stage.as_str(), "draft" | "review")).count();
    let tiles = if latest.is_some() {
        format!(
            r#"<div class="tiles"><div class="tile pass"><div class="k">Passed</div><div class="v">{}</div></div><div class="tile{}"><div class="k">Failed</div><div class="v">{}</div></div><div class="tile pend"><div class="k">Pending</div><div class="v">{}</div></div><div class="tile review"><div class="k">In review</div><div class="v">{reviewing}</div></div></div>"#,
            n("passed"),
            if n("failed") > 0 { " fail" } else { "" },
            n("failed"),
            n("pending")
        )
    } else {
        String::new()
    };
    let title = match latest {
        Some(_) if n("failed") > 0 => format!(r#"{} — <em class="hl" style="color:#FFB59E">{} failing</em>"#, esc(&project.name), n("failed")),
        Some(r) if r.executed && n("passed") > 0 => format!(r#"{} — <em class="hl">all green</em>"#, esc(&project.name)),
        _ => esc(&project.name),
    };
    let lede = match latest {
        Some(r) => format!("Latest run {} by {}{}.", ago(r.created_at), r.by, r.branch.as_deref().map(|b| format!(" on {b}")).unwrap_or_default()),
        None if !flows.is_empty() => "No runs in the last 30 days. The flow documents are below.".to_string(),
        None => "No runs yet. Publish one from your laptop or CI.".to_string(),
    };
    let mut body = String::new();
    if !flows.is_empty() {
        // Grouped by area, like the report.
        body.push_str(r#"<h2 class="sec">Flows</h2>"#);
        let mut groups: Vec<(String, Vec<&FlowRow>)> = Vec::new();
        for f in &flows {
            let area = f.area.clone().unwrap_or_else(|| "General".into());
            match groups.last_mut() {
                Some((a, list)) if *a == area => list.push(f),
                _ => groups.push((area, vec![f])),
            }
        }
        for (area, list) in &groups {
            let mut rows = String::new();
            let (mut passed, mut failed) = (0, 0);
            for f in list {
                let in_run = latest.and_then(|r| r.flows.iter().find(|x| x["id"].as_str() == Some(f.id.as_str())).map(|x| (r.id, x)));
                let status = in_run.and_then(|(_, x)| x["status"].as_str()).unwrap_or("not-run");
                match status {
                    "passed" => passed += 1,
                    "failed" => failed += 1,
                    _ => {}
                }
                let chip = if matches!(f.stage.as_str(), "draft" | "review" | "approved") && status != "failed" { f.stage.as_str() } else { status };
                let steps = in_run.and_then(|(_, x)| x["steps"].as_array());
                let dots: String = steps.map(|a| a.iter().map(|s| format!(r#"<i class="{}"></i>"#, esc(s.as_str().unwrap_or("")))).collect()).unwrap_or_default();
                let sub = match steps {
                    Some(a) => format!("{} steps", a.len()),
                    None => "not in the latest run".into(),
                };
                let err = in_run.and_then(|(_, x)| x["error"].as_str()).map(|e| format!(r#"<span class="small" style="display:block;color:var(--fail)">{}</span>"#, esc(e))).unwrap_or_default();
                let href = match in_run {
                    Some((rid, _)) => format!("/{}/runs/{rid}/#flow={}", project.slug, pages::urlenc(&f.id)),
                    None => format!("/{}/flows/#flow={}", project.slug, pages::urlenc(&f.id)),
                };
                let open = open_comments_on(app, project, &f.id)?;
                rows.push_str(&format!(
                    r#"<a class="row{}" href="{}"><span class="t"><b>{}</b><span class="small muted">{}</span>{err}</span><span class="dots" aria-hidden="true">{dots}</span>{}<span class="chip {}">{}</span></a>"#,
                    if status == "failed" { " failed" } else { "" },
                    esc(&href),
                    esc(&f.title),
                    esc(&sub),
                    if open > 0 { format!(r#"<span class="pill amber">{open} open</span>"#) } else { String::new() },
                    esc(chip),
                    esc(&chip.replace('-', " "))
                ));
            }
            let tally = match (passed, failed) {
                (_, f) if f > 0 => format!(r#"<span class="chip failed">{f} failed</span>"#),
                (p, _) if p == list.len() => r#"<span class="chip passed">all passed</span>"#.to_string(),
                _ => String::new(),
            };
            body.push_str(&format!(
                r#"<section class="group" aria-label="{a}"><div style="display:flex;align-items:baseline;gap:12px;margin:6px 0 10px"><h3 class="serif" style="font-size:20px">{a}</h3><span class="small muted">{n} flow{s}</span>{tally}</div><div class="list">{rows}</div></section>"#,
                a = esc(area),
                n = list.len(),
                s = if list.len() == 1 { "" } else { "s" },
            ));
        }
    }
    if !runs.is_empty() {
        body.push_str(r#"<h2 class="sec">Runs</h2><div class="table"><table aria-label="Runs, newest first"><thead><tr><th scope="col">When</th><th scope="col">Result</th><th scope="col">From</th><th scope="col">By</th></tr></thead><tbody>"#);
        for r in &runs {
            let f = r.summary.get("failed").and_then(Value::as_u64).unwrap_or(0);
            let p = r.summary.get("passed").and_then(Value::as_u64).unwrap_or(0);
            let kind = if r.prototype {
                "prototype"
            } else if r.executed {
                "run"
            } else {
                "docs"
            };
            body.push_str(&format!(
                r#"<tr><td><a href="/{}/runs/{}/">{}</a></td><td>{}</td><td>{}{}</td><td>{}</td></tr>"#,
                esc(&project.slug),
                r.id,
                esc(&ago(r.created_at)),
                if f > 0 { format!(r#"<span class="chip failed">{f} failed</span> {p} passed"#) } else { format!(r#"<span class="chip passed">{p} passed</span>"#) },
                esc(&r.source),
                esc(&format!(" · {kind}{}", r.branch.as_deref().map(|b| format!(" · {b}")).unwrap_or_default())),
                esc(&r.by)
            ));
        }
        body.push_str("</tbody></table></div>");
    }
    if flows.is_empty() && runs.is_empty() {
        body.push_str(&format!(r#"<div class="empty">Nothing published yet. <a href="/{}/publish">Send the first run</a>.</div>"#, esc(&project.slug)));
    } else {
        body.push_str(&format!(r#"<p class="small muted">Runs are kept for {} days. Flow documents and comments are kept.</p>"#, app.cfg.run_retention_days));
    }
    let projects = visible_projects(app, user)?;
    let site = app.cfg.site();
    let nav = nav(app, &site, user, &projects, project, "flows")?;
    let head = pages::head(&format!("{} · Flows and runs", project.name), &title, &lede, &tiles);
    Ok(axum::response::Html(pages::shell(&nav, &format!("{} · {site}", project.name), &format!(r#"{head}<div class="body">{body}</div>"#))).into_response())
}

pub async fn review_page(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>) -> R {
    let user = require_user(&app, &h, &format!("/{slug}/review"))?;
    let (project, _) = project_for(&app, &user, &slug)?;
    let mut list = String::new();
    for f in flows_of(&app, &project)?.iter().filter(|f| matches!(f.stage.as_str(), "draft" | "review")) {
        let open = open_comments_on(&app, &project, &f.id)?;
        list.push_str(&format!(
            r#"<a class="row" href="/{}/review/{}"><span class="t"><b>{}</b><span class="small muted">{}</span></span>{}<span class="chip {stage}">{stage}</span></a>"#,
            esc(&project.slug),
            esc(&pages::urlenc(&f.id)),
            esc(&f.title),
            esc(f.area.as_deref().unwrap_or("")),
            if open > 0 { format!(r#"<span class="pill amber">{open} open</span>"#) } else { String::new() },
            stage = esc(&f.stage)
        ));
    }
    if list.is_empty() {
        list = r#"<div class="empty">No drafts waiting for review.</div>"#.into();
    }
    let projects = visible_projects(&app, &user)?;
    let site = app.cfg.site();
    let nav = nav(&app, &site, &user, &projects, &project, "review")?;
    let head =
        pages::head(&format!("{} · Review", project.name), r#"Drafts waiting for <em class="hl">your eyes</em>"#, "Approval stays in git: set stage: approved and approved_by in the flow file.", "");
    Ok(axum::response::Html(pages::shell(&nav, &format!("Review · {}", project.name), &format!(r#"{head}<div class="body"><div class="list">{list}</div></div>"#))).into_response())
}

pub async fn flows_redirect(Path(slug): Path<String>) -> Response {
    Redirect::permanent(&format!("/{}/flows/", pages::urlenc(&slug))).into_response()
}

/// The flow documents as they were last published, with comments: what reviewers read.
pub async fn flows_page(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>) -> R {
    let user = require_user(&app, &h, &format!("/{slug}/flows/"))?;
    let (project, _) = project_for(&app, &user, &slug)?;
    let (docs, theme): (Vec<String>, Option<String>) = app.db.with(|c| {
        let mut st = c.prepare("SELECT doc FROM flows WHERE project_id = ?1 ORDER BY COALESCE(area, ''), title")?;
        let docs = st.query_map([project.id], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        let theme = c.query_row("SELECT theme FROM projects WHERE id = ?1", [project.id], |r| r.get(0))?;
        Ok((docs, theme))
    })?;
    let flows: Vec<enocone_spec::results::FlowResult> = docs
        .iter()
        .filter_map(|d| serde_json::from_str(d).ok())
        .map(|flow| enocone_spec::results::FlowResult { flow, status: Status::NotRun, skip_reason: None, duration_ms: 0, attempts: vec![] })
        .collect();
    let results = RunResults {
        schema: RESULTS_SCHEMA.into(),
        run: enocone_spec::results::RunInfo { id: "docs".into(), project: project.name.clone(), executed: false, ..Default::default() },
        summary: enocone_spec::results::Summary::from_flows(&flows),
        flows,
        theme,
    };
    report_page(&app, &project, &results, &app.cfg.data)
}

/// A report page for `results`, with the project's comments, under a CSP that allows only the report's own script.
fn report_page(app: &App, project: &Project, results: &RunResults, assets: &FsPath) -> R {
    let comments: Vec<Value> = app.db.with(|c| {
        let mut st = c.prepare("SELECT c.flow_id, c.step_key, c.text, c.created_at, c.resolved_at, u.name, u.github_login, u.email FROM comments c JOIN users u ON u.id = c.author_id WHERE c.project_id = ?1 ORDER BY c.created_at")?;
        st.query_map([project.id], |r| {
            let author: Option<String> = r.get::<_, Option<String>>(5)?.or(r.get(6)?).or(r.get(7)?);
            Ok(json!({ "flow": r.get::<_, String>(0)?, "step": r.get::<_, Option<String>>(1)?, "text": r.get::<_, String>(2)?, "at": r.get::<_, String>(3)?, "resolved": r.get::<_, Option<String>>(4)?.is_some(), "author": author }))
        })?
        .collect()
    })?;
    let mut extra = serde_json::Map::new();
    extra.insert("comments".into(), Value::Array(comments));
    extra.insert("hub".into(), json!({ "site": app.cfg.site(), "home": "/", "project": project.name, "project_url": format!("/{}", project.slug), "review_url": format!("/{}/review", project.slug), "retention_days": app.cfg.run_retention_days }));
    let html = enocone_report::render_html_with(results, assets, &enocone_report::ReportOptions { inline_assets: false }, extra)?;
    let hash = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(enocone_report::inline_script().as_bytes()));
    let csp = format!(
        "default-src 'self'; script-src 'sha256-{hash}'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'none'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'"
    );
    let mut res = axum::response::Html(html).into_response();
    res.headers_mut().insert("content-security-policy", HeaderValue::from_str(&csp)?);
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    Ok(res)
}

fn parse_run(run: &str) -> Result<i64, Fail> {
    run.parse().map_err(|_| Fail::NotFound)
}

fn run_exists(app: &App, project: &Project, run: i64) -> Result<(), Fail> {
    let found: Option<i64> = app.db.with(|c| c.query_row("SELECT id FROM runs WHERE id = ?1 AND project_id = ?2", params![run, project.id], |r| r.get(0)).optional())?;
    found.map(|_| ()).ok_or(Fail::NotFound)
}

pub async fn run_redirect(Path((slug, run)): Path<(String, String)>) -> Response {
    Redirect::permanent(&format!("/{}/runs/{}/", pages::urlenc(&slug), pages::urlenc(&run))).into_response()
}

pub async fn run_page(State(app): State<App>, h: HeaderMap, Path((slug, run)): Path<(String, String)>) -> R {
    let user = require_user(&app, &h, &format!("/{slug}/runs/{run}/"))?;
    let (project, _) = project_for(&app, &user, &slug)?;
    let run = parse_run(&run)?;
    run_exists(&app, &project, run)?;
    let dir = run_dir(&app, project.id, run);
    let results: RunResults = serde_json::from_slice(&std::fs::read(dir.join("results.json"))?)?;
    report_page(&app, &project, &results, &dir)
}

pub async fn run_asset(State(app): State<App>, h: HeaderMap, Path((slug, run, path)): Path<(String, String, String)>) -> R {
    let user = require_user(&app, &h, &format!("/{slug}"))?;
    let (project, _) = project_for(&app, &user, &slug)?;
    let run = parse_run(&run)?;
    let rel = format!("assets/{path}");
    if !safe_asset_path(&rel) {
        return Err(Fail::NotFound);
    }
    run_exists(&app, &project, run)?;
    let file: PathBuf = run_dir(&app, project.id, run).join(FsPath::new(&rel));
    let bytes = std::fs::read(&file).map_err(|_| Fail::NotFound)?;
    Ok(([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "private, max-age=86400")], bytes).into_response())
}

// ------------------------------------------------------------------ publishing page and tokens

struct TokenRow {
    id: i64,
    name: String,
    created_at: String,
    last_used_at: Option<String>,
}

fn tokens_of(app: &App, user: &User) -> anyhow::Result<Vec<TokenRow>> {
    app.db.with(|c| {
        let mut st = c.prepare("SELECT id, name, created_at, last_used_at FROM tokens WHERE user_id = ?1 ORDER BY id DESC")?;
        st.query_map([user.id], |r| Ok(TokenRow { id: r.get(0)?, name: r.get(1)?, created_at: r.get(2)?, last_used_at: r.get(3)? }))?.collect()
    })
}

fn publish_body(app: &App, user: &User, project: &Project, new_token: Option<&str>) -> anyhow::Result<String> {
    let origin = esc(&app.cfg.origin());
    let slug = esc(&project.slug);
    let repo_note = match &project.repo {
        Some(r) => format!("No secret to store: GitHub vouches for the workflow, and only {} is accepted.", esc(r)),
        None => "Link a GitHub repository to this project so CI can publish without a secret. Until then, set ENOCONE_HUB_TOKEN as a repository secret.".to_string(),
    };
    let fresh = new_token
        .map(|t| format!(r#"<div class="notice iris" role="status"><b>New token: copy it now, it won't be shown again.</b><pre class="term" style="margin-top:8px">{}</pre></div>"#, esc(t)))
        .unwrap_or_default();
    let mut rows = String::new();
    for t in tokens_of(app, user)? {
        let used = t.last_used_at.as_deref().map(|u| format!("last used {}", u.replace('T', " ").trim_end_matches('Z'))).unwrap_or_else(|| "never used".into());
        rows.push_str(&format!(
            r#"<div class="row"><span style="width:10px;height:10px;border-radius:50%;background:{}"></span><span class="t"><b style="font-family:Inter,sans-serif;font-size:15px">{}</b><span class="small muted">created {}</span></span><span class="small">{}</span><form method="post" action="/tokens/{}/revoke"><input type="hidden" name="back" value="/{slug}/publish"><button class="btn small danger" type="submit" aria-label="Revoke {}">Revoke</button></form></div>"#,
            if t.last_used_at.is_some() { "#15803D" } else { "#B7791F" },
            esc(&t.name),
            esc(t.created_at.split('T').next().unwrap_or("")),
            esc(&used),
            t.id,
            esc(&t.name)
        ));
    }
    if rows.is_empty() {
        rows = r#"<div class="empty">No tokens yet. <code>enocone login</code> makes one for this computer.</div>"#.into();
    }
    Ok(format!(
        r#"{fresh}<div class="grid2">
<section class="panel" aria-labelledby="cli-h"><h2 id="cli-h" class="serif" style="font-size:22px">From your laptop</h2><p class="small" style="margin:0">Sign in once; the token stays in your system keychain.</p>
<pre class="term"><span class="c"># enocone.toml</span>
<span class="k">[hub]</span>
url = <span class="s">"{origin}"</span>
project = <span class="s">"{slug}"</span>

<span class="p">$</span> enocone login
<span class="p">$</span> enocone run
<span class="p">$</span> enocone publish</pre></section>
<section class="panel" aria-labelledby="ci-h"><h2 id="ci-h" class="serif" style="font-size:22px">From GitHub Actions</h2><p class="small" style="margin:0">{repo_note}</p>
<pre class="term"><span class="k">permissions:</span>
  contents: read
  id-token: write

<span class="k">steps:</span>
  - uses: actions/checkout@v4
  - uses: driftwerk/enocone@v1
    with:
      project: e2e
      publish: <span class="s">true</span></pre></section></div>
<section aria-labelledby="tok-h" style="display:flex;flex-direction:column;gap:12px"><h2 id="tok-h" class="sec">Your publish tokens</h2>
<div class="list">{rows}</div>
<form method="post" action="/tokens" style="display:flex;flex-wrap:wrap;gap:10px;align-items:flex-end"><input type="hidden" name="back" value="/{slug}/publish"><div class="field" style="flex:1 1 240px;max-width:360px"><label for="tname">New token for</label><input id="tname" name="name" type="text" required maxlength="60" placeholder="build server"></div><button class="btn ghost" type="submit">Create token</button></form>
<p class="small muted" style="margin:0">Runs are kept for {days} days. Flow documents and comments are kept.</p></section>"#,
        days = app.cfg.run_retention_days
    ))
}

pub async fn publish_page(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>) -> R {
    render_publish(&app, &h, &slug, None)
}

fn render_publish(app: &App, h: &HeaderMap, slug: &str, new_token: Option<&str>) -> R {
    let user = require_user(app, h, &format!("/{slug}/publish"))?;
    let (project, _) = project_for(app, &user, slug)?;
    let projects = visible_projects(app, &user)?;
    let site = app.cfg.site();
    let nav = nav(app, &site, &user, &projects, &project, "publish")?;
    let head = pages::head(
        &format!("{} · Publishing", project.name),
        r#"Send runs here from <em class="hl">a laptop</em> or <em class="hl">from CI</em>"#,
        "A run sends its results and screenshots, never HTML: this site draws every page itself.",
        "",
    );
    let body = publish_body(app, &user, &project, new_token)?;
    Ok(axum::response::Html(pages::shell(&nav, &format!("Publishing · {}", project.name), &format!(r#"{head}<div class="body">{body}</div>"#))).into_response())
}

#[derive(Deserialize)]
pub struct NewToken {
    name: String,
    back: Option<String>,
}

pub async fn new_token(State(app): State<App>, h: HeaderMap, Form(f): Form<NewToken>) -> R {
    same_origin(&app, &h)?;
    let user = require_user(&app, &h, "/")?;
    let name: String = f.name.trim().chars().take(60).collect();
    if name.is_empty() {
        return Err(Fail::Bad("Give the token a name.".into()));
    }
    let token = format!("enc_{}", auth::random_token());
    app.db.with(|c| c.execute("INSERT INTO tokens (user_id, name, token_hash, created_at) VALUES (?1, ?2, ?3, ?4)", params![user.id, name, auth::hash(&token), db::now()]))?;
    let slug = f.back.as_deref().and_then(|b| b.strip_prefix('/')).and_then(|b| b.strip_suffix("/publish")).unwrap_or("");
    if slug.is_empty() {
        return Ok(axum::response::Html(pages::message(&app.cfg.site(), "New publish token", &token, Some(("/", "Done")))).into_response());
    }
    let mut res = render_publish(&app, &h, slug, Some(&token))?;
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

#[derive(Deserialize)]
pub struct Back {
    back: Option<String>,
}

pub async fn revoke_token(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): Form<Back>) -> R {
    same_origin(&app, &h)?;
    let user = require_user(&app, &h, "/")?;
    app.db.with(|c| c.execute("DELETE FROM tokens WHERE id = ?1 AND user_id = ?2", params![id, user.id]))?;
    let back = f.back.filter(|b| b.starts_with('/') && !b.starts_with("//")).unwrap_or_else(|| "/".into());
    Ok(Redirect::to(&back).into_response())
}

#[cfg(test)]
mod tests {
    use super::safe_asset_path;

    #[test]
    fn asset_paths_are_guarded() {
        assert!(safe_asset_path("assets/checkout/1-3.png"));
        assert!(!safe_asset_path("assets/../enocone.db"));
        assert!(!safe_asset_path("/etc/passwd.png"));
        assert!(!safe_asset_path("assets/x.html"));
        assert!(!safe_asset_path("assets/.hidden.png"));
        assert!(!safe_asset_path("assets//x.png"));
    }
}
