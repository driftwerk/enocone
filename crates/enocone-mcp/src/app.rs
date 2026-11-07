//! Shared orchestration used by the CLI and the MCP server: pick flows,
//! start a browser only when needed, run, and write every output.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use enocone_driver_playwright::{LaunchConfig, PlaywrightDriver};
use enocone_engine::config::AuthProfile;
use enocone_engine::{Driver, Engine, EventSink, Project, RunOptions};
use enocone_report::ReportOptions;
use enocone_spec::{BlockKind, Diagnostic, Flow, RunResults, Severity};

#[derive(Debug, Clone, Default)]
pub struct RunRequest {
    pub paths: Vec<PathBuf>,
    pub env: Option<String>,
    pub tags: Vec<String>,
    pub areas: Vec<String>,
    pub ids: Vec<String>,
    pub grep: Option<String>,
    pub workers: Option<usize>,
    pub retries: Option<u32>,
    pub headed: bool,
    pub slow_mo_ms: Option<f64>,
    pub browser: Option<String>,
    pub channel: Option<String>,
    pub out_dir: PathBuf,
    pub inline_assets: bool,
    /// Run browser steps against the flows' prototype instead of the real app.
    pub prototype: bool,
}

pub enum RunError {
    /// Flow documents have errors (exit code 2).
    Spec(Vec<(String, Diagnostic)>),
    /// Couldn't start (browser, config…) (exit code 3).
    Infra(anyhow::Error),
}

impl From<anyhow::Error> for RunError {
    fn from(e: anyhow::Error) -> Self {
        RunError::Infra(e)
    }
}

pub struct RunOutput {
    pub results: RunResults,
    pub report: PathBuf,
    pub warnings: Vec<(String, Diagnostic)>,
}

pub fn select(flows: Vec<Flow>, req: &RunRequest) -> Vec<Flow> {
    flows
        .into_iter()
        .filter(|f| req.tags.is_empty() || req.tags.iter().any(|t| f.meta.tags.contains(t)))
        .filter(|f| req.areas.is_empty() || req.areas.iter().any(|a| f.area().eq_ignore_ascii_case(a)))
        .filter(|f| req.ids.is_empty() || req.ids.iter().any(|i| &f.id == i || f.id.starts_with(&format!("{i}."))))
        .filter(|f| {
            req.grep.as_ref().is_none_or(|g| {
                let g = g.to_lowercase();
                f.title.to_lowercase().contains(&g) || f.id.to_lowercase().contains(&g)
            })
        })
        .collect()
}

/// Does anything in these flows need a browser?
pub fn needs_browser(project: &Project, flows: &[Flow]) -> bool {
    needs_browser_for(project, flows, false)
}

pub fn needs_browser_for(project: &Project, flows: &[Flow], prototype: bool) -> bool {
    let browser_profiles: Vec<&String> = project.config.auth.iter().filter(|(_, p)| matches!(p, AuthProfile::Browser { .. })).map(|(k, _)| k).collect();
    flows.iter().filter(|f| prototype || f.meta.stage.runs()).any(|f| {
        f.uses_browser()
            || f.steps()
                .flat_map(|s| s.blocks.iter())
                .any(|b| b.kind == BlockKind::Use || (b.kind == BlockKind::Http && b.value.get("auth").and_then(|a| a.as_str()).is_some_and(|a| browser_profiles.iter().any(|p| p.as_str() == a))))
    })
}

pub fn git_info(root: &Path) -> (Option<String>, Option<String>, Option<String>) {
    let git = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git").args(args).current_dir(root).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
    };
    let branch = std::env::var("GITHUB_HEAD_REF").ok().filter(|s| !s.is_empty()).or_else(|| std::env::var("GITHUB_REF_NAME").ok()).or_else(|| git(&["rev-parse", "--abbrev-ref", "HEAD"]));
    let commit = std::env::var("GITHUB_SHA").ok().or_else(|| git(&["rev-parse", "HEAD"]));
    let ci = match (std::env::var("GITHUB_SERVER_URL"), std::env::var("GITHUB_REPOSITORY"), std::env::var("GITHUB_RUN_ID")) {
        (Ok(s), Ok(r), Ok(id)) => Some(format!("{s}/{r}/actions/runs/{id}")),
        _ => std::env::var("CI_JOB_URL").ok(),
    };
    (branch, commit, ci)
}

/// Write index.html, results.json, junit.xml and summary.md into `out_dir`.
pub fn write_outputs(results: &RunResults, out_dir: &Path, inline_assets: bool) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(out_dir)?;
    std::fs::write(out_dir.join("results.json"), serde_json::to_string_pretty(results)?)?;
    if results.run.executed {
        std::fs::write(out_dir.join("junit.xml"), enocone_report::junit(results))?;
        std::fs::write(out_dir.join("summary.md"), enocone_report::markdown_summary(results, None))?;
    }
    enocone_report::write_html(results, out_dir, &ReportOptions { inline_assets })
}

pub async fn run(project: &Project, req: &RunRequest, events: EventSink) -> Result<RunOutput, RunError> {
    let (flows, diags) = project.load_flows(&req.paths)?;
    let (errors, warnings): (Vec<_>, Vec<_>) = diags.into_iter().partition(|(_, d)| d.severity == Severity::Error);
    if !errors.is_empty() {
        return Err(RunError::Spec(errors));
    }
    let flows = select(flows, req);
    let env = project.pick_env(req.env.as_deref())?;
    let mut opts = RunOptions::from_project(project, env, req.out_dir.clone())?;
    if let Some(w) = req.workers {
        opts.workers = w.max(1);
    }
    if let Some(r) = req.retries {
        opts.retries = r;
    }
    let server = if req.prototype {
        let proto = crate::prototype::Prototype::from_flows(project, &flows);
        let (addr, handle) = crate::prototype::serve(proto, ([127, 0, 0, 1], 0).into()).await?;
        opts.prototype = Some(format!("http://{addr}"));
        Some(handle)
    } else {
        None
    };
    // Fresh output folder (keep nothing stale from a previous run).
    let assets = req.out_dir.join("assets");
    if assets.exists() {
        let _ = std::fs::remove_dir_all(&assets);
    }
    std::fs::create_dir_all(&req.out_dir).map_err(anyhow::Error::from)?;

    let driver: Option<Arc<dyn Driver>> = if needs_browser_for(project, &flows, req.prototype) {
        let rc = &project.config.run;
        let cfg = LaunchConfig { browser: req.browser.clone().unwrap_or_else(|| rc.browser.clone()), headless: !req.headed && rc.headless, slow_mo_ms: req.slow_mo_ms, channel: req.channel.clone() };
        Some(Arc::new(PlaywrightDriver::launch(&cfg).await?))
    } else {
        None
    };
    let engine = Arc::new(Engine::new(project.clone(), opts, driver, events)?);
    let mut results = engine.run(flows).await;
    engine.shutdown().await;
    if let Some(h) = server {
        h.abort();
    }
    let (branch, commit, ci) = git_info(&project.root);
    results.run.git_branch = branch;
    results.run.git_commit = commit;
    results.run.ci_url = ci;
    results.theme = project.theme_css();
    let report = write_outputs(&results, &req.out_dir, req.inline_assets)?;
    Ok(RunOutput { results, report, warnings })
}

/// Documentation view of the flows (nothing runs).
pub fn docs(project: &Project, paths: &[PathBuf], out_dir: &Path) -> Result<(PathBuf, Vec<(String, Diagnostic)>), RunError> {
    let (flows, diags) = project.load_flows(paths)?;
    let mut results = RunResults::docs(&project.config.project, flows);
    results.theme = project.theme_css();
    let path = write_outputs(&results, out_dir, false)?;
    Ok((path, diags))
}

/// A mockup as a full HTML page with the project's theme, the same way the
/// report's sandboxed frame shows it.
pub fn screen_document(html: &str, theme: Option<&str>) -> String {
    let base = format!("<meta charset=\"utf-8\"><style>html,body{{margin:0}}body{{font-family:Inter,system-ui,sans-serif}}</style><style>{}</style>", theme.unwrap_or(""));
    let lower = html.to_ascii_lowercase();
    let head = lower.match_indices("<head").map(|(i, _)| i).find(|&i| matches!(lower.as_bytes().get(i + 5), Some(b'>' | b' ' | b'\t' | b'\n' | b'\r')));
    if let Some(i) = head
        && let Some(j) = lower[i..].find('>')
    {
        let at = i + j + 1;
        return format!("{}{base}{}", &html[..at], &html[at..]);
    }
    format!("<!doctype html><html><head>{base}</head><body>{html}</body></html>")
}

/// Where a screen comes from: a name in the screens folder, a path to an
/// .html file, or inline HTML.
pub enum ScreenSource {
    Name(String),
    Html(String),
}

/// Draw a mockup with the project's theme in a headless browser and return a PNG.
/// Scripts are turned off, as in the report.
pub async fn render_screen(project: &Project, source: &ScreenSource) -> anyhow::Result<Vec<u8>> {
    let (label, html) = match source {
        ScreenSource::Html(h) => ("inline".to_string(), h.clone()),
        ScreenSource::Name(n) => {
            let as_path = project.root.join(n);
            let file = if n.ends_with(".html") && as_path.is_file() { as_path } else { project.root.join(&project.config.screens).join(format!("{n}.html")) };
            let html = std::fs::read_to_string(&file).map_err(|_| anyhow::anyhow!("screen `{n}` not found: expected {}", file.display()))?;
            (n.trim_end_matches(".html").replace(['/', '\\'], "-"), html)
        }
    };
    let doc = screen_document(&html, project.theme_css().as_deref());
    // No scripts: the report's frame is sandboxed, so the picture must match.
    let doc = doc.replacen("<meta charset=\"utf-8\">", "<meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"script-src 'none'\">", 1);
    let dir = project.root.join(".enocone/screens");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!("{label}.html"));
    std::fs::write(&file, doc)?;
    let rc = &project.config.run;
    let driver = PlaywrightDriver::launch(&LaunchConfig { browser: rc.browser.clone(), headless: true, slow_mo_ms: None, channel: None })
        .await
        .map_err(|e| anyhow::anyhow!("could not start the browser: {e:#} (try `enocone install-browsers`)"))?;
    let result = async {
        let session = driver.new_session(Default::default()).await?;
        let url = format!("file://{}", file.canonicalize()?.display());
        session.goto(&url, std::time::Duration::from_secs(15)).await?;
        let png = session.screenshot().await;
        let _ = session.close().await;
        png
    }
    .await;
    let _ = driver.shutdown().await;
    result
}
