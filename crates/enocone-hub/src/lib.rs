//! `enocone serve`: the team site (enoc.one, or your own) where flow
//! documents, runs and review comments are shared.
//!
//! One binary, one data folder: SQLite for people, projects and comments, and
//! files for runs and screenshots. It serves plain HTTP on a local port; put
//! Caddy or another proxy in front for HTTPS.

mod auth;
mod db;
mod github;
mod mail;
mod oidc;
mod pages;
mod review;
mod runs;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Form, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;

use crate::auth::Limiter;
use crate::db::{Db, Project, Role, User};
use crate::pages::esc;

/// Everything `enocone serve` needs; the CLI fills it from flags and `ENOCONE_*` variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// e.g. `https://enoc.one`; links in emails and cookie security follow it.
    pub public_url: String,
    pub listen: SocketAddr,
    pub data: PathBuf,
    /// The site owner: may always sign in, sees and creates every project.
    pub owner_email: Option<String>,
    /// The site owner's GitHub login, as an alternative to the email.
    pub owner_github: Option<String>,
    pub github_client_id: Option<String>,
    pub github_client_secret: Option<String>,
    /// github.com and its API (overridable so tests can point them at a stand-in).
    pub github_url: String,
    pub github_api_url: String,
    pub postmark_token: Option<String>,
    /// Postmark's API (overridable so tests can point it at a stand-in).
    pub postmark_url: String,
    pub mail_from: String,
    pub run_retention_days: u32,
    /// Who signs CI tokens (GitHub Actions; overridable for tests).
    pub oidc_issuer: String,
}

pub use oidc::GITHUB_ISSUER;

impl Config {
    fn origin(&self) -> String {
        let u = self.public_url.trim_end_matches('/');
        match u.find("://").and_then(|i| u[i + 3..].find('/').map(|j| i + 3 + j)) {
            Some(end) => u[..end].to_string(),
            None => u.to_string(),
        }
    }
    fn secure(&self) -> bool {
        self.public_url.starts_with("https://")
    }
    fn site(&self) -> String {
        let o = self.origin();
        o.split("://").nth(1).unwrap_or(&o).to_string()
    }
    fn is_owner(&self, u: &User) -> bool {
        let by_email = matches!((&self.owner_email, &u.email), (Some(o), Some(e)) if o.eq_ignore_ascii_case(e));
        let by_login = matches!((&self.owner_github, &u.github_login), (Some(o), Some(l)) if o.eq_ignore_ascii_case(l));
        by_email || by_login
    }
}

#[derive(Clone)]
pub struct App {
    cfg: Arc<Config>,
    db: Db,
    mailer: mail::Mailer,
    limits: Arc<(Limiter, Limiter)>,
    oidc: Arc<oidc::Verifier>,
}

impl App {
    pub fn new(cfg: Config) -> anyhow::Result<App> {
        std::fs::create_dir_all(&cfg.data)?;
        std::fs::create_dir_all(cfg.data.join("runs"))?;
        let db = Db::open(&cfg.data.join("enocone.db"))?;
        let mailer = mail::Mailer::new(cfg.postmark_url.clone(), cfg.postmark_token.clone(), cfg.mail_from.clone());
        // Sign-in links: 3 per address and 10 per IP in 15 minutes.
        let limits = Arc::new((Limiter::new(3, Duration::from_secs(900)), Limiter::new(10, Duration::from_secs(900))));
        let oidc = Arc::new(oidc::Verifier::new(cfg.oidc_issuer.clone()));
        Ok(App { cfg: Arc::new(cfg), db, mailer, limits, oidc })
    }

    /// A one-time sign-in link for `email`, for the first sign-in on a server with no mail set up.
    pub fn sign_in_link(&self, email: &str) -> anyhow::Result<String> {
        let email = email.trim().to_lowercase();
        let token = auth::random_token();
        let exp = db::epoch() + auth::LINK_MINUTES * 60;
        self.db.with(|c| c.execute("INSERT INTO email_links (token_hash, email, expires_at) VALUES (?1, ?2, ?3)", params![auth::hash(&token), email, exp]))?;
        Ok(format!("{}/auth/email?token={token}", self.cfg.origin()))
    }

    /// A publish token for `email` (who must have signed in before), named `name`.
    pub fn create_token(&self, email: &str, name: &str) -> anyhow::Result<String> {
        let email = email.trim().to_lowercase();
        let token = format!("enc_{}", auth::random_token());
        self.db.with(|c| {
            let u = db::user_for_email(c, &email)?;
            c.execute("INSERT INTO tokens (user_id, name, token_hash, created_at) VALUES (?1, ?2, ?3, ?4)", params![u.id, name, auth::hash(&token), db::now()])
        })?;
        Ok(token)
    }

    /// Delete expired runs now, as if the clock were `offset_days` ahead (0 in real use).
    pub fn sweep_now(&self, offset_days: i64) -> anyhow::Result<usize> {
        runs::sweep(self, db::epoch() + offset_days * 86400)
    }

    pub fn router(self) -> Router {
        Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .route("/assets/hub.css", get(|| async { ([(header::CONTENT_TYPE, "text/css; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], pages::HUB_CSS) }))
            .route("/favicon.svg", get(|| async { ([(header::CONTENT_TYPE, "image/svg+xml")], pages::FAVICON) }))
            .route("/login", get(login_page))
            .route("/login/email", post(login_email))
            .route("/login/sent", get(login_sent))
            .route("/auth/email", get(auth_email_page).post(auth_email))
            .route("/logout", post(logout))
            .route("/auth/github", get(github::start))
            .route("/auth/github/callback", get(github::callback))
            .route("/", get(home))
            .route("/projects", post(create_project))
            .route("/tokens", post(runs::new_token))
            .route("/cli/authorize", get(cli_authorize_page).post(cli_authorize))
            .route("/tokens/{id}/revoke", post(runs::revoke_token))
            .route("/api/whoami", get(runs::whoami))
            .route("/api/projects/{slug}/flows/{flow_id}/comments", get(review::api_comments))
            .route("/api/projects/{slug}/runs", post(runs::publish).get(runs::list_runs).layer(DefaultBodyLimit::max(runs::MAX_UPLOAD)))
            .route("/{slug}", get(project_page))
            .route("/{slug}/members", get(members_page).post(members_add))
            .route("/{slug}/members/remove", post(members_remove))
            .route("/{slug}/publish", get(runs::publish_page))
            .route("/{slug}/review", get(runs::review_page))
            .route("/{slug}/review/{flow}", get(review::flow_review_page))
            .route("/{slug}/review/{flow}/comments", post(review::add_comment))
            .route("/{slug}/comments/{id}/resolve", post(review::resolve))
            .route("/{slug}/flows", get(runs::flows_redirect))
            .route("/{slug}/flows/", get(runs::flows_page))
            .route("/{slug}/runs/{run}", get(runs::run_redirect))
            .route("/{slug}/runs/{run}/", get(runs::run_page))
            .route("/{slug}/runs/{run}/assets/{*path}", get(runs::run_asset))
            .layer(axum::middleware::from_fn(security_headers))
            .with_state(self)
    }
}

/// Run the site until the process is stopped. Old runs are deleted hourly.
pub async fn serve(cfg: Config) -> anyhow::Result<()> {
    let listen = cfg.listen;
    let app = App::new(cfg)?;
    let sweeper = app.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = runs::sweep(&sweeper, db::epoch()) {
                tracing::warn!("retention sweep failed: {e:#}");
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
    let listener = tokio::net::TcpListener::bind(listen).await?;
    eprintln!("enocone serve: {} on http://{}", app.cfg.public_url, listener.local_addr()?);
    axum::serve(listener, app.router().into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}

async fn security_headers(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("referrer-policy", HeaderValue::from_static("same-origin"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    if !h.contains_key("strict-transport-security") {
        h.insert("strict-transport-security", HeaderValue::from_static("max-age=31536000"));
    }
    if !h.contains_key("content-security-policy") {
        h.insert(
            "content-security-policy",
            HeaderValue::from_static("default-src 'self'; script-src 'none'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; form-action 'self'; frame-ancestors 'none'; base-uri 'none'"),
        );
    }
    res
}

// ------------------------------------------------------------------ helpers

pub(crate) enum Fail {
    NotFound,
    Forbidden(&'static str),
    Login(String),
    Bad(String),
    Internal(anyhow::Error),
}

impl<E: Into<anyhow::Error>> From<E> for Fail {
    fn from(e: E) -> Fail {
        Fail::Internal(e.into())
    }
}

impl IntoResponse for Fail {
    fn into_response(self) -> Response {
        match self {
            Fail::NotFound => (StatusCode::NOT_FOUND, Html(pages::doc("Not found", r#"<div class="cover gate"><main class="card"><div class="kicker">404</div><h1 class="serif" style="font-size:30px">Not found</h1><p class="lede" style="font-size:17px">There is nothing here, or you don't have access to it.</p><a class="btn ghost" href="/">Your projects</a></main></div>"#))).into_response(),
            Fail::Forbidden(why) => (StatusCode::FORBIDDEN, Html(pages::doc("Not allowed", &format!(r#"<div class="cover gate"><main class="card"><div class="kicker">Not allowed</div><p class="lede">{}</p></main></div>"#, esc(why))))).into_response(),
            Fail::Login(next) => Redirect::to(&format!("/login?next={}", pages::urlenc(&next))).into_response(),
            Fail::Bad(m) => (StatusCode::BAD_REQUEST, Html(pages::doc("Bad request", &format!(r#"<div class="cover gate"><main class="card"><div class="kicker">Bad request</div><p class="lede">{}</p></main></div>"#, esc(&m))))).into_response(),
            Fail::Internal(e) => {
                tracing::error!("{e:#}");
                eprintln!("[enocone serve] error: {e:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into_response()
            }
        }
    }
}

pub(crate) type R = Result<Response, Fail>;

/// The signed-in person, from the session cookie.
pub(crate) fn current_user(app: &App, h: &HeaderMap) -> anyhow::Result<Option<User>> {
    let Some(tok) = auth::cookie(h, auth::SESSION_COOKIE) else { return Ok(None) };
    app.db.with(|c| {
        let uid: Option<i64> = c.query_row("SELECT user_id FROM sessions WHERE token_hash = ?1 AND expires_at > ?2", params![auth::hash(&tok), db::epoch()], |r| r.get(0)).optional()?;
        match uid {
            Some(id) => db::user_by_id(c, id),
            None => Ok(None),
        }
    })
}

pub(crate) fn require_user(app: &App, h: &HeaderMap, here: &str) -> Result<User, Fail> {
    current_user(app, h)?.ok_or_else(|| Fail::Login(here.to_string()))
}

/// Forms must come from this site: the browser's Origin (or Referer) must match.
pub(crate) fn same_origin(app: &App, h: &HeaderMap) -> Result<(), Fail> {
    let origin = app.cfg.origin();
    let ok = match h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        Some(o) => o == origin,
        None => h.get(header::REFERER).and_then(|v| v.to_str().ok()).is_some_and(|r| r == origin || r.starts_with(&format!("{origin}/"))),
    };
    if ok { Ok(()) } else { Err(Fail::Forbidden("This form was sent from another site.")) }
}

/// A project the user may see, with their role (the site owner is an owner everywhere). Otherwise 404.
pub(crate) fn project_for(app: &App, user: &User, slug: &str) -> Result<(Project, Role), Fail> {
    let found = app.db.with(|c| {
        let Some(p) = db::project_by_slug(c, slug)? else { return Ok(None) };
        let role = if app.cfg.is_owner(user) { Some(Role::Owner) } else { db::role_of(c, p.id, user)? };
        Ok(role.map(|r| (p, r)))
    })?;
    found.ok_or(Fail::NotFound)
}

pub(crate) fn visible_projects(app: &App, user: &User) -> anyhow::Result<Vec<Project>> {
    app.db.with(|c| {
        let all = db::all_projects(c)?;
        if app.cfg.is_owner(user) {
            return Ok(all);
        }
        let mut out = Vec::new();
        for p in all {
            if db::role_of(c, p.id, user)?.is_some() {
                out.push(p);
            }
        }
        Ok(out)
    })
}

fn html(s: String) -> Response {
    Html(s).into_response()
}

fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(n) if n.starts_with('/') && !n.starts_with("//") && !n.contains('\\') => n.to_string(),
        _ => "/".to_string(),
    }
}

fn client_ip(peer: SocketAddr, h: &HeaderMap) -> String {
    // Behind a local proxy (Caddy), the client is the last X-Forwarded-For hop.
    if peer.ip().is_loopback()
        && let Some(x) = h.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(last) = x.split(',').map(str::trim).rfind(|s| !s.is_empty())
    {
        return last.to_string();
    }
    peer.ip().to_string()
}

// ------------------------------------------------------------------ sign-in

#[derive(Deserialize)]
struct NextQ {
    next: Option<String>,
    error: Option<String>,
}

async fn login_page(State(app): State<App>, h: HeaderMap, Query(q): Query<NextQ>) -> R {
    if current_user(&app, &h)?.is_some() {
        return Ok(Redirect::to(&safe_next(q.next.as_deref())).into_response());
    }
    let github = app.cfg.github_client_id.is_some() && app.cfg.github_client_secret.is_some();
    Ok(html(pages::home(&app.cfg.site(), &app.cfg.origin(), github, app.cfg.run_retention_days, &safe_next(q.next.as_deref()), q.error.as_deref())))
}

#[derive(Deserialize)]
struct EmailForm {
    email: String,
    next: Option<String>,
}

async fn login_email(State(app): State<App>, ConnectInfo(peer): ConnectInfo<SocketAddr>, h: HeaderMap, Form(f): Form<EmailForm>) -> R {
    same_origin(&app, &h)?;
    let email = f.email.trim().to_lowercase();
    if !email.contains('@') || email.len() > 254 {
        return Ok(Redirect::to("/login?error=That+doesn%27t+look+like+an+email+address.").into_response());
    }
    let sent_page = Redirect::to(&format!("/login/sent?to={}", pages::urlenc(&email))).into_response();
    let (per_email, per_ip) = &*app.limits;
    if !per_ip.allow(&client_ip(peer, &h)) || !per_email.allow(&email) {
        // Same answer either way: nothing reveals whether the address is invited.
        return Ok(sent_page);
    }
    let owner = app.cfg.owner_email.as_deref().is_some_and(|o| o.eq_ignore_ascii_case(&email));
    let allowed = owner || app.db.with(|c| db::email_may_sign_in(c, &email))?;
    if allowed {
        let token = auth::random_token();
        let exp = db::epoch() + auth::LINK_MINUTES * 60;
        let next = safe_next(f.next.as_deref());
        app.db.with(|c| c.execute("INSERT INTO email_links (token_hash, email, expires_at, next) VALUES (?1, ?2, ?3, ?4)", params![auth::hash(&token), email, exp, next]))?;
        let link = format!("{}/auth/email?token={token}", app.cfg.origin());
        let site = app.cfg.site();
        let text = format!("Sign in to {site}:\n\n{link}\n\nThe link works once and expires in {} minutes. If you didn't ask for it, ignore this email.\n", auth::LINK_MINUTES);
        let body = format!(
            r#"<p>Sign in to <strong>{site}</strong>:</p><p><a href="{link}" style="display:inline-block;background:#5B3DF0;color:#ffffff;border-radius:8px;padding:10px 18px;text-decoration:none;font-weight:600">Sign in to {site}</a></p><p style="color:#5F6678">The link works once and expires in {} minutes. If you didn't ask for it, ignore this email.</p>"#,
            auth::LINK_MINUTES,
            site = esc(&site),
            link = esc(&link)
        );
        // Sent in the background, so an invited address answers as fast as a stranger's.
        let mailer = app.mailer.clone();
        tokio::spawn(async move {
            if let Err(e) = mailer.send(&email, &format!("Your sign-in link for {site}"), &text, &body).await {
                tracing::error!("{e:#}");
                eprintln!("[enocone serve] {e:#}");
            }
        });
    }
    Ok(sent_page)
}

#[derive(Deserialize)]
struct ToQ {
    to: Option<String>,
}

async fn login_sent(State(app): State<App>, Query(q): Query<ToQ>) -> Response {
    html(pages::link_sent(&app.cfg.site(), q.to.as_deref().unwrap_or("your address")))
}

#[derive(Deserialize)]
struct TokenQ {
    token: String,
}

enum LinkState {
    Ok(String, Option<String>),
    Used,
    Expired,
    Unknown,
}

fn link_state(app: &App, token: &str) -> anyhow::Result<LinkState> {
    let row: Option<(String, i64, Option<i64>, Option<String>)> = app.db.with(|c| {
        c.query_row("SELECT email, expires_at, used_at, next FROM email_links WHERE token_hash = ?1", [auth::hash(token)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional()
    })?;
    Ok(match row {
        None => LinkState::Unknown,
        Some((_, _, Some(_), _)) => LinkState::Used,
        Some((_, exp, None, _)) if exp <= db::epoch() => LinkState::Expired,
        Some((email, _, None, next)) => LinkState::Ok(email, next),
    })
}

fn link_problem(app: &App, s: &LinkState) -> Option<Response> {
    let site = app.cfg.site();
    let page = |title: &str, text: &str| (StatusCode::GONE, Html(pages::message(&site, title, text, Some(("/login", "Get a new link"))))).into_response();
    match s {
        LinkState::Ok(..) => None,
        LinkState::Used => Some(page("This link has already been used", "Sign-in links work once. Ask for a new one.")),
        LinkState::Expired => Some(page("This link has expired", "Sign-in links last 15 minutes. Ask for a new one.")),
        LinkState::Unknown => Some(page("This sign-in link isn't valid", "Copy the whole link from the email, or ask for a new one.")),
    }
}

async fn auth_email_page(State(app): State<App>, Query(q): Query<TokenQ>) -> R {
    let s = link_state(&app, &q.token)?;
    if let Some(r) = link_problem(&app, &s) {
        return Ok(r);
    }
    let LinkState::Ok(email, _) = s else { unreachable!() };
    Ok(html(pages::confirm_link(&app.cfg.site(), &q.token, &email)))
}

async fn auth_email(State(app): State<App>, h: HeaderMap, Form(f): Form<TokenQ>) -> R {
    same_origin(&app, &h)?;
    let s = link_state(&app, &f.token)?;
    if let Some(r) = link_problem(&app, &s) {
        return Ok(r);
    }
    let LinkState::Ok(email, next) = s else { unreachable!() };
    // Use the link exactly once, even if two requests race.
    let claimed = app.db.with(|c| c.execute("UPDATE email_links SET used_at = ?1 WHERE token_hash = ?2 AND used_at IS NULL", params![db::epoch(), auth::hash(&f.token)]))?;
    if claimed != 1 {
        return Ok(link_problem(&app, &LinkState::Used).unwrap());
    }
    let user = app.db.with(|c| db::user_for_email(c, &email))?;
    start_session(&app, &user, &safe_next(next.as_deref()))
}

pub(crate) fn start_session(app: &App, user: &User, next: &str) -> R {
    let token = auth::random_token();
    let exp = db::epoch() + auth::SESSION_DAYS * 86400;
    app.db.with(|c| c.execute("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?1, ?2, ?3)", params![auth::hash(&token), user.id, exp]))?;
    let mut res = Redirect::to(next).into_response();
    res.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&auth::session_cookie(&token, app.cfg.secure()))?);
    Ok(res)
}

async fn logout(State(app): State<App>, h: HeaderMap) -> R {
    same_origin(&app, &h)?;
    if let Some(tok) = auth::cookie(&h, auth::SESSION_COOKIE) {
        app.db.with(|c| c.execute("DELETE FROM sessions WHERE token_hash = ?1", [auth::hash(&tok)]))?;
    }
    let mut res = Redirect::to("/").into_response();
    res.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&auth::clear_cookie(app.cfg.secure()))?);
    Ok(res)
}

// ------------------------------------------------------------------ enocone login

#[derive(Deserialize)]
struct CliAuth {
    port: u16,
    state: String,
    name: Option<String>,
}

fn cli_auth_valid(q: &CliAuth) -> bool {
    q.port >= 1024 && !q.state.is_empty() && q.state.len() <= 128 && q.state.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// `enocone login` opens this page; allowing it hands a new token to the CLI on a local port.
async fn cli_authorize_page(State(app): State<App>, h: HeaderMap, Query(q): Query<CliAuth>) -> R {
    let here = format!("/cli/authorize?port={}&state={}&name={}", q.port, q.state, pages::urlenc(q.name.as_deref().unwrap_or("")));
    let user = require_user(&app, &h, &here)?;
    if !cli_auth_valid(&q) {
        return Err(Fail::Bad("This sign-in link from enocone login is incomplete. Run enocone login again.".into()));
    }
    let name = q.name.clone().unwrap_or_else(|| "enocone login".into());
    let body = format!(
        r#"<div class="cover gate"><main class="card">
  <div class="kicker">enocone login</div>
  <h1 class="serif" style="font-size:30px;line-height:1.1">Let enocone on <em style="color:var(--iris)">{name}</em> publish runs as you?</h1>
  <p class="lede" style="font-size:17px">Signed in as {who}. The token can publish to projects where you are a member or owner, and can be revoked on any project's Publishing page.</p>
  <form method="post" action="/cli/authorize"><input type="hidden" name="port" value="{port}"><input type="hidden" name="state" value="{state}"><input type="hidden" name="name" value="{name}"><button class="btn iris wide" type="submit">Allow</button></form>
  <a class="btn ghost" href="/">Cancel</a>
</main></div>"#,
        name = esc(&name),
        who = esc(&user.label()),
        port = q.port,
        state = esc(&q.state)
    );
    let mut res = html(pages::doc(&format!("enocone login · {}", app.cfg.site()), &body));
    res.headers_mut().insert(
        "content-security-policy",
        HeaderValue::from_static("default-src 'self'; script-src 'none'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; form-action 'self' http://127.0.0.1:*; frame-ancestors 'none'; base-uri 'none'"),
    );
    Ok(res)
}

async fn cli_authorize(State(app): State<App>, h: HeaderMap, Form(q): Form<CliAuth>) -> R {
    same_origin(&app, &h)?;
    let user = require_user(&app, &h, "/")?;
    if !cli_auth_valid(&q) {
        return Err(Fail::Bad("This sign-in link from enocone login is incomplete. Run enocone login again.".into()));
    }
    let name: String = q.name.as_deref().unwrap_or("enocone login").chars().take(60).collect();
    let token = format!("enc_{}", auth::random_token());
    app.db.with(|c| c.execute("INSERT INTO tokens (user_id, name, token_hash, created_at) VALUES (?1, ?2, ?3, ?4)", params![user.id, name, auth::hash(&token), db::now()]))?;
    Ok(Redirect::to(&format!("http://127.0.0.1:{}/callback?state={}&token={token}", q.port, q.state)).into_response())
}

// ------------------------------------------------------------------ projects

async fn home(State(app): State<App>, h: HeaderMap) -> R {
    let Some(user) = current_user(&app, &h)? else {
        let github = app.cfg.github_client_id.is_some() && app.cfg.github_client_secret.is_some();
        return Ok(html(pages::home(&app.cfg.site(), &app.cfg.origin(), github, app.cfg.run_retention_days, "/", None)));
    };
    let projects = visible_projects(&app, &user)?;
    let owner = app.cfg.is_owner(&user);
    let mut list = String::new();
    for p in &projects {
        list.push_str(&format!(
            r#"<a class="row" href="/{}"><span class="t"><b>{}</b><span class="small muted">{}</span></span></a>"#,
            esc(&p.slug),
            esc(&p.name),
            esc(p.repo.as_deref().map(|r| format!("github.com/{r}")).as_deref().unwrap_or(&p.slug))
        ));
    }
    if projects.is_empty() {
        list = format!(
            r#"<div class="empty">{}</div>"#,
            if owner { "No projects yet. Create the first one below." } else { "You haven't been added to a project yet. Ask a project owner to invite you." }
        );
    }
    let create = if owner {
        r#"<section class="panel" aria-labelledby="new-h"><h2 id="new-h" class="sec">New project</h2>
<form method="post" action="/projects" style="display:flex;flex-wrap:wrap;gap:12px;align-items:flex-end">
<div class="field" style="flex:1 1 180px"><label for="slug">Short name</label><input id="slug" name="slug" type="text" required pattern="[a-z0-9][a-z0-9-]{0,39}" placeholder="shop"></div>
<div class="field" style="flex:2 1 220px"><label for="name">Display name</label><input id="name" name="name" type="text" required placeholder="Shop"></div>
<div class="field" style="flex:2 1 220px"><label for="repo">GitHub repository (optional)</label><input id="repo" name="repo" type="text" placeholder="acme/shop"></div>
<button class="btn iris" type="submit">Create project</button></form></section>"#
    } else {
        ""
    };
    let nav = pages::Nav { site: &app.cfg.site(), user: &user, projects: &projects, current: None, section: "", review_count: 0 };
    let head = pages::head(&app.cfg.site(), r#"Your <em class="hl">projects</em>"#, "Flow documents, runs and review comments, shared with your team.", "");
    Ok(html(pages::shell(&nav, &format!("Projects · {}", app.cfg.site()), &format!(r#"{head}<div class="body"><div class="list">{list}</div>{create}</div>"#))))
}

#[derive(Deserialize)]
struct NewProject {
    slug: String,
    name: String,
    repo: Option<String>,
}

pub(crate) fn valid_slug(s: &str) -> bool {
    let reserved = ["login", "logout", "auth", "api", "assets", "projects", "tokens", "healthz", "favicon.svg", "cli"];
    !s.is_empty() && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !s.starts_with('-') && !reserved.contains(&s)
}

async fn create_project(State(app): State<App>, h: HeaderMap, Form(f): Form<NewProject>) -> R {
    same_origin(&app, &h)?;
    let user = require_user(&app, &h, "/")?;
    if !app.cfg.is_owner(&user) {
        return Err(Fail::Forbidden("Only the site owner can create projects."));
    }
    let slug = f.slug.trim().to_lowercase();
    if !valid_slug(&slug) {
        return Err(Fail::Bad("A short name uses lower-case letters, digits and dashes, up to 40 characters.".into()));
    }
    let repo = f.repo.map(|r| r.trim().trim_start_matches("https://github.com/").trim_matches('/').to_lowercase()).filter(|r| !r.is_empty());
    if let Some(r) = &repo
        && !(r.split('/').count() == 2 && r.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b)))
    {
        return Err(Fail::Bad("The repository should look like owner/name.".into()));
    }
    let name = f.name.trim().to_string();
    let created = app.db.with(|c| {
        if db::project_by_slug(c, &slug)?.is_some() {
            return Ok(false);
        }
        c.execute("INSERT INTO projects (slug, name, repo, created_at) VALUES (?1, ?2, ?3, ?4)", params![slug, if name.is_empty() { &slug } else { &name }, repo, db::now()])?;
        let id = c.last_insert_rowid();
        if let Some(e) = &user.email {
            c.execute("INSERT INTO members (project_id, kind, subject, role, created_at) VALUES (?1, 'email', ?2, 'owner', ?3)", params![id, e, db::now()])?;
        }
        Ok(true)
    })?;
    if !created {
        return Err(Fail::Bad(format!("There is already a project called {slug}.")));
    }
    Ok(Redirect::to(&format!("/{slug}")).into_response())
}

async fn project_page(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>) -> R {
    let user = require_user(&app, &h, &format!("/{slug}"))?;
    let (project, _role) = project_for(&app, &user, &slug)?;
    runs::project_page(&app, &user, &project)
}

// ------------------------------------------------------------------ members

async fn members_page(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>) -> R {
    let user = require_user(&app, &h, &format!("/{slug}/members"))?;
    let (project, role) = project_for(&app, &user, &slug)?;
    let rows: Vec<(String, String, String)> = app.db.with(|c| {
        let mut st = c.prepare("SELECT kind, subject, role FROM members WHERE project_id = ?1 ORDER BY CASE role WHEN 'owner' THEN 0 WHEN 'member' THEN 1 ELSE 2 END, subject")?;
        st.query_map([project.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect()
    })?;
    let can_manage = role == Role::Owner;
    let mut list = String::new();
    for (kind, subject, r) in &rows {
        let r = Role::parse(r).unwrap_or(Role::Reviewer);
        let (initial, how) = match kind.as_str() {
            "team" => ("◆".to_string(), "GitHub team · checked at each sign-in. If the organisation restricts OAuth apps, an org owner must approve this site first."),
            "github" => (subject.chars().next().unwrap_or('?').to_uppercase().to_string(), "GitHub"),
            _ => (subject.chars().next().unwrap_or('?').to_uppercase().to_string(), "email link or GitHub"),
        };
        let shown = if kind == "github" { format!("@{subject}") } else { subject.clone() };
        let remove = if can_manage {
            format!(
                r#"<form method="post" action="/{slug}/members/remove"><input type="hidden" name="kind" value="{kind}"><input type="hidden" name="subject" value="{subj}"><button class="btn small danger" type="submit" aria-label="Remove {subj}">Remove</button></form>"#,
                slug = esc(&project.slug),
                kind = esc(kind),
                subj = esc(subject)
            )
        } else {
            String::new()
        };
        list.push_str(&format!(r#"<div class="row"><span class="avatar">{}</span><span class="t"><b style="font-family:Inter,sans-serif;font-size:15px">{}</b><span class="small muted">{how}</span></span>{}{remove}</div>"#, esc(&initial), esc(&shown), pages::role_pill(r)));
    }
    let invite = if can_manage {
        format!(
            r#"<form class="invite" method="post" action="/{}/members">
<div class="field"><label for="who">Invite a person or a GitHub team</label><input id="who" name="who" type="text" required placeholder="email, @handle or org/team"></div>
<div class="field role"><label for="role">Role</label><select id="role" name="role"><option value="reviewer">Reviewer</option><option value="member">Member</option><option value="owner">Owner</option></select></div>
<button class="btn iris" type="submit">Send invite</button></form>"#,
            esc(&project.slug)
        )
    } else {
        String::new()
    };
    let projects = visible_projects(&app, &user)?;
    let nav = pages::Nav { site: &app.cfg.site(), user: &user, projects: &projects, current: Some(&project), section: "members", review_count: runs::open_comment_count(&app, &project)? };
    let head = pages::head(
        &format!("{} · Members", project.name),
        &format!(r#"Who can see <em class="hl">{}</em>"#, esc(&project.name)),
        "Private to the people and GitHub teams below. Everyone else gets “not found”.",
        &invite,
    );
    let roles = r#"<div class="grid2" style="grid-template-columns:repeat(auto-fit,minmax(200px,1fr))"><div class="panel" style="background:var(--ink);color:#fff;border:0"><b>Owner</b><span class="small" style="color:var(--lav2)">Members, roles, linked repository</span></div><div class="panel" style="background:var(--iris-bg);border:0;color:#2E2280"><b>Member</b><span class="small">Publishes runs, comments, resolves</span></div><div class="panel" style="background:var(--amber-bg);border:0;color:#5A3B06"><b>Reviewer</b><span class="small">Reads and comments</span></div></div>"#;
    Ok(html(pages::shell(&nav, &format!("Members · {}", project.name), &format!(r#"{head}<div class="body">{roles}<div class="list" aria-label="Members">{list}</div></div>"#))))
}

#[derive(Deserialize)]
struct InviteForm {
    who: String,
    role: String,
}

async fn members_add(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>, Form(f): Form<InviteForm>) -> R {
    same_origin(&app, &h)?;
    let user = require_user(&app, &h, &format!("/{slug}/members"))?;
    let (project, role) = project_for(&app, &user, &slug)?;
    if role != Role::Owner {
        return Err(Fail::Forbidden("Only project owners can invite people."));
    }
    let r = Role::parse(&f.role).ok_or_else(|| Fail::Bad("Unknown role.".into()))?;
    let who = f.who.trim().to_lowercase();
    let (kind, subject) = if who.contains('@') && !who.starts_with('@') {
        ("email", who.clone())
    } else if let Some(login) = who.strip_prefix('@') {
        ("github", login.to_string())
    } else if who.split('/').count() == 2 {
        ("team", who.clone())
    } else {
        ("github", who.clone())
    };
    if subject.is_empty() || subject.len() > 254 {
        return Err(Fail::Bad("Enter an email address, a GitHub @handle or an org/team.".into()));
    }
    app.db.with(|c| {
        c.execute(
            "INSERT INTO members (project_id, kind, subject, role, created_at) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT (project_id, kind, subject) DO UPDATE SET role = excluded.role",
            params![project.id, kind, subject, r.as_str(), db::now()],
        )
    })?;
    if kind == "email" {
        let site = app.cfg.site();
        let link = format!("{}/login", app.cfg.origin());
        let text = format!("{} invited you to {} on {site} as a {}.\n\nSign in with this address: {link}\n", user.label(), project.name, r.as_str());
        let body = format!(
            r#"<p>{} invited you to <strong>{}</strong> on {} as a {}.</p><p><a href="{}">Sign in with this address</a></p>"#,
            esc(&user.label()),
            esc(&project.name),
            esc(&site),
            r.as_str(),
            esc(&link)
        );
        if let Err(e) = app.mailer.send(&subject, &format!("You're invited to {} on {site}", project.name), &text, &body).await {
            eprintln!("[enocone serve] {e:#}");
        }
    }
    Ok(Redirect::to(&format!("/{}/members", project.slug)).into_response())
}

#[derive(Deserialize)]
struct RemoveForm {
    kind: String,
    subject: String,
}

async fn members_remove(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>, Form(f): Form<RemoveForm>) -> R {
    same_origin(&app, &h)?;
    let user = require_user(&app, &h, &format!("/{slug}/members"))?;
    let (project, role) = project_for(&app, &user, &slug)?;
    if role != Role::Owner {
        return Err(Fail::Forbidden("Only project owners can remove people."));
    }
    let ok = app.db.with(|c| {
        let owners: i64 = c.query_row("SELECT COUNT(*) FROM members WHERE project_id = ?1 AND role = 'owner'", [project.id], |r| r.get(0))?;
        let is_owner: bool =
            c.query_row("SELECT COUNT(*) FROM members WHERE project_id = ?1 AND kind = ?2 AND subject = ?3 AND role = 'owner'", params![project.id, f.kind, f.subject], |r| r.get::<_, i64>(0))? > 0;
        if is_owner && owners <= 1 {
            return Ok(false);
        }
        c.execute("DELETE FROM members WHERE project_id = ?1 AND kind = ?2 AND subject = ?3", params![project.id, f.kind, f.subject])?;
        // Their publish tokens stop working for this project at once: tokens are checked against membership on every use.
        Ok(true)
    })?;
    if !ok {
        return Err(Fail::Bad("A project needs at least one owner.".into()));
    }
    Ok(Redirect::to(&format!("/{}/members", project.slug)).into_response())
}
