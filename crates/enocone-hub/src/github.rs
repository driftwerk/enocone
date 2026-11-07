//! "Continue with GitHub": OAuth sign-in and GitHub team grants.
//!
//! Scopes: read:user, user:email and read:org (to read team membership),
//! never repository access. Teams are read at each sign-in. Only verified
//! GitHub emails are matched against email invites. Someone with no grant
//! anywhere gets no account and no session.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;

use crate::db::{self, User};
use crate::{App, Fail, R, auth, pages, safe_next, start_session};

const STATE_COOKIE: &str = "enocone_oauth";
pub const SCOPES: &str = "read:user user:email read:org";

#[derive(Deserialize)]
pub struct Start {
    next: Option<String>,
}

pub async fn start(State(app): State<App>, Query(q): Query<Start>) -> R {
    let (Some(id), Some(_)) = (&app.cfg.github_client_id, &app.cfg.github_client_secret) else { return Err(Fail::NotFound) };
    let state = auth::random_token();
    let next = safe_next(q.next.as_deref());
    let url = format!(
        "{}/login/oauth/authorize?client_id={}&redirect_uri={}&scope={}&state={state}&allow_signup=false",
        app.cfg.github_url,
        pages::urlenc(id),
        pages::urlenc(&format!("{}/auth/github/callback", app.cfg.origin())),
        pages::urlenc(SCOPES)
    );
    let cookie =
        format!("{STATE_COOKIE}={state}.{}; Path=/auth/github; HttpOnly; SameSite=Lax; Max-Age=600{}", pages::urlenc(&next), if app.cfg.public_url.starts_with("https://") { "; Secure" } else { "" });
    let mut res = Redirect::to(&url).into_response();
    res.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&cookie)?);
    Ok(res)
}

#[derive(Deserialize)]
pub struct Callback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct GhUser {
    login: String,
    id: i64,
    name: Option<String>,
}

#[derive(Deserialize)]
struct GhEmail {
    email: String,
    verified: bool,
    #[serde(default)]
    primary: bool,
}

#[derive(Deserialize)]
struct GhOrg {
    login: String,
}

#[derive(Deserialize)]
struct GhTeam {
    slug: String,
    organization: GhOrg,
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 3 <= b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn refuse(app: &App, title: &str, text: &str) -> R {
    Ok((StatusCode::FORBIDDEN, Html(pages::message(&app.cfg.site(), title, text, Some(("/login", "Back to sign-in"))))).into_response())
}

pub async fn callback(State(app): State<App>, h: HeaderMap, Query(q): Query<Callback>) -> R {
    let (Some(client_id), Some(secret)) = (&app.cfg.github_client_id, &app.cfg.github_client_secret) else { return Err(Fail::NotFound) };
    if q.error.is_some() {
        return refuse(&app, "GitHub sign-in was cancelled", "Nothing was shared. Try again, or ask for an email link.");
    }
    let cookie = auth::cookie(&h, STATE_COOKIE).unwrap_or_default();
    let (want, next) = cookie.split_once('.').unwrap_or(("", "/"));
    let (Some(code), Some(state)) = (q.code, q.state) else { return Err(Fail::Bad("GitHub didn't send a code back.".into())) };
    if want.is_empty() || want != state {
        return refuse(&app, "This sign-in expired", "Start again from the sign-in page (sign-ins through GitHub must finish within 10 minutes).");
    }
    let next = safe_next(Some(&percent_decode(next)));

    let http = reqwest::Client::builder().user_agent("enocone-serve").timeout(std::time::Duration::from_secs(15)).build()?;
    #[derive(Deserialize)]
    struct Tok {
        access_token: Option<String>,
        error: Option<String>,
    }
    let tok: Tok = http
        .post(format!("{}/login/oauth/access_token", app.cfg.github_url))
        .header("Accept", "application/json")
        .form(&[("client_id", client_id.as_str()), ("client_secret", secret.as_str()), ("code", code.as_str()), ("redirect_uri", &format!("{}/auth/github/callback", app.cfg.origin()))])
        .send()
        .await?
        .json()
        .await?;
    let Some(token) = tok.access_token else {
        tracing::warn!("GitHub token exchange failed: {:?}", tok.error);
        return refuse(&app, "GitHub sign-in didn't work", "GitHub refused the sign-in. Try again, or ask for an email link.");
    };
    let api = |path: &str| http.get(format!("{}{path}", app.cfg.github_api_url)).bearer_auth(&token).header("Accept", "application/vnd.github+json");
    let gh: GhUser = api("/user").send().await?.error_for_status()?.json().await?;
    let emails: Vec<GhEmail> = api("/user/emails").send().await?.error_for_status()?.json().await.unwrap_or_default();
    let mut teams: Vec<String> = Vec::new();
    for page in 1..=10 {
        let batch: Vec<GhTeam> = api(&format!("/user/teams?per_page=100&page={page}")).send().await?.error_for_status()?.json().await.unwrap_or_default();
        let n = batch.len();
        teams.extend(batch.into_iter().map(|t| format!("{}/{}", t.organization.login, t.slug).to_lowercase()));
        if n < 100 {
            break;
        }
    }
    let verified: Vec<String> = emails.iter().filter(|e| e.verified).map(|e| e.email.to_lowercase()).collect();
    let primary = emails.iter().find(|e| e.verified && e.primary).map(|e| e.email.to_lowercase()).or_else(|| verified.first().cloned());
    let login = gh.login.to_lowercase();

    // Is there any grant for this identity? (Nothing is written before this check.)
    let owner = app.cfg.owner_email.as_deref().is_some_and(|o| verified.iter().any(|e| e.eq_ignore_ascii_case(o)));
    let allowed = owner
        || app.db.with(|c| {
            let mut n: i64 = c.query_row("SELECT COUNT(*) FROM members WHERE kind = 'github' AND subject = ?1", [&login], |r| r.get(0))?;
            for e in &verified {
                n += c.query_row("SELECT COUNT(*) FROM members WHERE kind = 'email' AND subject = ?1", [e], |r| r.get::<_, i64>(0))?;
            }
            for t in &teams {
                n += c.query_row("SELECT COUNT(*) FROM members WHERE kind = 'team' AND subject = ?1", [t], |r| r.get::<_, i64>(0))?;
            }
            Ok(n > 0)
        })?;
    if !allowed {
        return refuse(&app, &format!("@{} isn't on any project here", gh.login), "Ask a project owner to invite your GitHub account, a verified email address on it, or one of your GitHub teams.");
    }

    let user: User = app.db.with(|c| {
        // The GitHub account, or else the person who was invited by one of its verified emails.
        let mut uid: Option<i64> = c.query_row("SELECT id FROM users WHERE github_id = ?1", [gh.id], |r| r.get(0)).optional()?;
        if uid.is_none() {
            for e in &verified {
                if let Some(id) = c.query_row("SELECT id FROM users WHERE email = ?1 AND github_id IS NULL", [e], |r| r.get(0)).optional()? {
                    uid = Some(id);
                    break;
                }
            }
        }
        let id = match uid {
            Some(id) => {
                c.execute("UPDATE users SET github_login = ?1, github_id = ?2, name = COALESCE(?3, name), email = COALESCE(email, ?4) WHERE id = ?5", params![login, gh.id, gh.name, primary, id])?;
                id
            }
            None => {
                // An email already used by another account is left off this one.
                let email = primary.as_deref().filter(|e| db::user_by_email(c, e).ok().flatten().is_none());
                c.execute("UPDATE users SET github_login = NULL WHERE github_login = ?1", [&login])?;
                c.execute("INSERT INTO users (email, github_login, github_id, name, created_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![email, login, gh.id, gh.name, db::now()])?;
                c.last_insert_rowid()
            }
        };
        c.execute("DELETE FROM user_teams WHERE user_id = ?1", [id])?;
        for t in &teams {
            c.execute("INSERT OR IGNORE INTO user_teams (user_id, team) VALUES (?1, ?2)", params![id, t])?;
        }
        Ok(db::user_by_id(c, id)?.expect("just written"))
    })?;
    let mut res = start_session(&app, &user, &next)?;
    res.headers_mut().append(header::SET_COOKIE, HeaderValue::from_str(&format!("{STATE_COOKIE}=; Path=/auth/github; HttpOnly; SameSite=Lax; Max-Age=0"))?);
    Ok(res)
}
