//! A tiny conveyancing-style app (leads, quotes, accept + pay links, login)
//! that enocone's own tests and the demo flows run against.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

pub const USER: &str = "alice";
pub const PASSWORD: &str = "secret";
pub const TOKEN: &str = "tok-alice-123456";
pub const API_KEY: &str = "agency-key-98765";
/// The emulated Postmark sandbox server's API token.
pub const POSTMARK_TOKEN: &str = "fixture-postmark-token";

#[derive(Default)]
struct Db {
    leads: HashMap<String, Value>,
    quotes: HashMap<String, Value>,
    /// Emails the app "sent". Readable through emulated Postmark and Mailpit APIs.
    emails: Vec<Email>,
    /// Short-lived access tokens: token → (serial, issued at).
    short_tokens: HashMap<String, (u64, std::time::Instant)>,
    /// Named resources a setup flow tidies up: id → {id, name, group}.
    resources: HashMap<String, Value>,
    /// Invites sent: (group, person).
    invites: Vec<(String, String)>,
}

/// How long a short-lived token lasts.
pub const SHORT_TOKEN_TTL: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Clone)]
struct Email {
    id: String,
    to: String,
    from: String,
    subject: String,
    text: String,
    html: String,
    sent_at: String,
}

type S = Arc<Mutex<Db>>;

fn id(prefix: &str) -> String {
    format!("{prefix}_{}", &uuid::Uuid::new_v4().simple().to_string()[..6])
}

fn page(title: &str, body: &str) -> Html<String> {
    Html(format!("<!doctype html><html lang=en><head><meta charset=utf-8><title>{title}</title></head><body><main>{body}</main></body></html>"))
}

fn authed(h: &HeaderMap) -> bool {
    let bearer = h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {TOKEN}"));
    let key = h.get("x-api-key").and_then(|v| v.to_str().ok()) == Some(API_KEY);
    bearer || key
}

fn signed_in(h: &HeaderMap) -> bool {
    h.get(header::COOKIE).and_then(|v| v.to_str().ok()).is_some_and(|c| c.contains(&format!("session={TOKEN}")))
}

fn unauthorised() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": "unauthorised" }))).into_response()
}

fn money(pence: i64) -> String {
    let pounds = pence / 100;
    let s = pounds.to_string();
    let mut with_commas = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            with_commas.push(',');
        }
        with_commas.push(c);
    }
    format!("£{with_commas}.{:02}", pence % 100)
}

pub fn app() -> Router {
    let state: S = Arc::default();
    Router::new()
        .route("/", get(home))
        .route("/login", get(login_page).post(login))
        .route("/api/auth/session", get(session))
        .route("/api/auth/short-session", get(short_session))
        .route("/api/auth/short-session-once", get(short_session_once))
        .route("/api/short/whoami", get(short_whoami))
        .route("/api/leads", post(create_lead))
        .route("/api/leads/{id}", get(get_lead))
        .route("/api/leads/{id}/close", post(close_lead))
        .route("/api/quotes", post(create_quote))
        .route("/api/discount", post(discount))
        .route("/api/orders", post(order_total))
        .route("/api/reports/{who}", get(report_access))
        .route("/api/people/{group}", get(people))
        .route("/api/invites/{group}", post(invite).get(invites))
        .route("/api/resources/{group}", post(create_resource).get(list_resources))
        .route("/api/resources/{group}/{id}", axum::routing::delete(delete_resource))
        .route("/api/quotes/{id}/payment-status", get(payment_status))
        .route("/public/quotes/{id}/accept/{token}", get(accept_page).post(accept))
        .route("/public/quotes/{id}/pay", get(pay_page).post(pay))
        .route("/pay-quote/{id}/payment-success", get(paid_page))
        .route("/lead/{id}/milestones", get(milestones))
        // Emulated sandbox mailboxes (read-only APIs, same shapes as the real ones).
        .route("/postmark/email", post(postmark_send))
        .route("/oidc/.well-known/openid-configuration", get(oidc_config))
        .route("/oidc/.well-known/jwks", get(oidc_jwks))
        .route("/oidc/token", get(oidc_token))
        .route("/github/login/oauth/authorize", get(gh_authorize_page).post(gh_authorize))
        .route("/github/login/oauth/access_token", post(gh_access_token))
        .route("/github/api/user", get(gh_user))
        .route("/github/api/user/emails", get(gh_emails))
        .route("/github/api/user/teams", get(gh_teams))
        .route("/postmark/messages/outbound", get(postmark_list))
        .route("/postmark/messages/outbound/{id}/details", get(postmark_details))
        .route("/mailpit/api/v1/search", get(mailpit_search))
        .route("/mailpit/api/v1/message/{id}", get(mailpit_message))
        .with_state(state)
}

/// Serve on `addr` (port 0 = any free port).
pub async fn serve(addr: SocketAddr) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app()).await.unwrap();
    });
    (addr, handle)
}

async fn home(h: HeaderMap) -> Response {
    if !signed_in(&h) {
        return Redirect::to("/login").into_response();
    }
    page("Dashboard", "<h1>Dashboard</h1><p>Signed in as alice</p>").into_response()
}

async fn login_page() -> Html<String> {
    page(
        "Sign in",
        r#"<h1>Sign in to your account</h1>
<form method="post" action="/login">
<label for="u">Username or email</label><input id="u" name="username">
<label for="p">Password</label><input id="p" name="password" type="password">
<button type="submit">Sign In</button></form>"#,
    )
}

#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
}

async fn login(Form(f): Form<LoginForm>) -> Response {
    if f.username == USER && f.password == PASSWORD {
        ([(header::SET_COOKIE, format!("session={TOKEN}; Path=/; HttpOnly"))], Redirect::to("/")).into_response()
    } else {
        page("Sign in", "<h1>Sign in to your account</h1><p role=alert>Invalid username or password.</p>").into_response()
    }
}

async fn session(h: HeaderMap) -> Json<Value> {
    if signed_in(&h) { Json(json!({ "user": { "name": USER }, "accessToken": TOKEN })) } else { Json(json!({})) }
}

async fn create_lead(State(s): State<S>, h: HeaderMap, Json(body): Json<Value>) -> Response {
    if !authed(&h) {
        return unauthorised();
    }
    if body.get("property").is_none() {
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": "property is required" }))).into_response();
    }
    let lead_id = id("ld");
    let mut lead = body.clone();
    lead["id"] = json!(lead_id);
    lead["status"] = json!("open");
    lead["milestones"] = json!({ "quote_accepted": false });
    s.lock().unwrap().leads.insert(lead_id.clone(), lead.clone());
    (StatusCode::CREATED, Json(lead)).into_response()
}

async fn get_lead(State(s): State<S>, h: HeaderMap, Path(lead): Path<String>) -> Response {
    if !authed(&h) {
        return unauthorised();
    }
    match s.lock().unwrap().leads.get(&lead) {
        Some(l) => Json(l.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response(),
    }
}

async fn close_lead(State(s): State<S>, h: HeaderMap, Path(lead): Path<String>) -> Response {
    if !authed(&h) {
        return unauthorised();
    }
    match s.lock().unwrap().leads.get_mut(&lead) {
        Some(l) => {
            l["status"] = json!("closed");
            Json(l.clone()).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn create_quote(State(s): State<S>, h: HeaderMap, Json(body): Json<Value>) -> Response {
    if !authed(&h) {
        return unauthorised();
    }
    let total: i64 = body["fees"]
        .as_array()
        .map(|fees| {
            fees.iter()
                .map(|f| {
                    let a = f["amount"].as_i64().unwrap_or(0);
                    if f["vat"].as_bool().unwrap_or(false) { a * 6 / 5 } else { a }
                })
                .sum()
        })
        .unwrap_or(0);
    let qid = id("q");
    let token = id("tk");
    let quote = json!({ "id": qid, "lead_id": body["lead_id"], "total": total, "status": "draft", "accept_token": token, "paid": false });
    let mut db = s.lock().unwrap();
    if let Some(to) = body["client_email"].as_str() {
        // Links point back at whichever address the API was called on.
        let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("127.0.0.1:4010");
        let link = format!("http://{host}/public/quotes/{qid}/accept/{token}");
        db.emails.push(Email {
            id: uuid::Uuid::new_v4().to_string(),
            to: to.to_string(),
            from: "quotes@example.test".into(),
            subject: format!("Your conveyancing quote: {}", money(total)),
            text: format!("Hello,\n\nYour quote comes to {}.\nAccept it here: {link}\n", money(total)),
            html: format!(
                r#"<p>Hello,</p><p>Your quote comes to <strong>{}</strong>.</p><p><a href="{link}">Review and accept your quote</a></p><p><a href="http://{host}/help">Questions?</a></p>"#,
                money(total)
            ),
            sent_at: now_rfc3339(),
        });
    }
    db.quotes.insert(qid, quote.clone());
    drop(db);
    (StatusCode::CREATED, Json(quote)).into_response()
}

async fn payment_status(State(s): State<S>, h: HeaderMap, Path(q): Path<String>) -> Response {
    if !authed(&h) {
        return unauthorised();
    }
    match s.lock().unwrap().quotes.get(&q) {
        Some(q) => Json(json!({ "status": if q["paid"].as_bool() == Some(true) { "paid" } else { "pending" } })).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn accept_page(State(s): State<S>, h: HeaderMap, Path((q, token)): Path<(String, String)>) -> Response {
    // This page's own address, URL-encoded inside another link, as sign-in links often carry it.
    let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("127.0.0.1");
    let here: String = format!("http://{host}/public/quotes/{q}/accept/{token}")
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect();
    let db = s.lock().unwrap();
    let Some(quote) = db.quotes.get(&q).filter(|q| q["accept_token"] == json!(token)) else {
        return (StatusCode::NOT_FOUND, page("Not found", "<h1>This link has expired</h1>")).into_response();
    };
    page(
        "Your quote",
        &format!(
            r#"<h1>Your conveyancing quote</h1><p>Total: {}</p><p>Not you? <a href="/login?next={here}">Sign in as someone else</a> <small>(/login?next={here})</small></p>
<form method="post"><label><input type="checkbox" name="terms" value="yes"> I accept the terms of engagement</label>
<button type="submit">Accept quote</button></form>"#,
            money(quote["total"].as_i64().unwrap_or(0)),
        ),
    )
    .into_response()
}

#[derive(Deserialize)]
struct AcceptForm {
    terms: Option<String>,
}

async fn accept(State(s): State<S>, Path((q, _token)): Path<(String, String)>, Form(f): Form<AcceptForm>) -> Response {
    if f.terms.is_none() {
        return page("Your quote", "<h1>Your conveyancing quote</h1><p role=alert>Please accept the terms to continue.</p>").into_response();
    }
    let mut db = s.lock().unwrap();
    if let Some(quote) = db.quotes.get_mut(&q) {
        quote["status"] = json!("accepted");
        let lead = quote["lead_id"].as_str().unwrap_or("").to_string();
        if let Some(l) = db.leads.get_mut(&lead) {
            l["milestones"]["quote_accepted"] = json!(true);
        }
    }
    Redirect::to(&format!("/public/quotes/{q}/pay")).into_response()
}

async fn pay_page(State(s): State<S>, Path(q): Path<String>) -> Response {
    let total = s.lock().unwrap().quotes.get(&q).and_then(|q| q["total"].as_i64()).unwrap_or(0);
    page("Pay", &format!(r#"<h1>Pay for your quote</h1><form method="post"><label for="c">Card number</label><input id="c" name="card"><button type="submit">Pay {}</button></form>"#, money(total)))
        .into_response()
}

async fn pay(State(s): State<S>, Path(q): Path<String>) -> Response {
    if let Some(quote) = s.lock().unwrap().quotes.get_mut(&q) {
        quote["paid"] = json!(true);
    }
    Redirect::to(&format!("/pay-quote/{q}/payment-success")).into_response()
}

async fn paid_page(State(s): State<S>, Path(q): Path<String>) -> Response {
    let total = s.lock().unwrap().quotes.get(&q).and_then(|q| q["total"].as_i64()).unwrap_or(0);
    page("Paid", &format!(r#"<section aria-label="Payment confirmed"><h1>Payment confirmed</h1><p>We have received {}.</p></section>"#, money(total))).into_response()
}

async fn milestones(State(s): State<S>, h: HeaderMap, Path(lead): Path<String>) -> Response {
    if !signed_in(&h) {
        return Redirect::to("/login").into_response();
    }
    let accepted = s.lock().unwrap().leads.get(&lead).map(|l| l["milestones"]["quote_accepted"] == json!(true)).unwrap_or(false);
    page("Milestones", &format!(r#"<h1>Milestones</h1><label><input type="checkbox" disabled {}> Quote accepted</label>"#, if accepted { "checked" } else { "" })).into_response()
}

// ---------------------------------------------------------------- emulated mailboxes

fn now_rfc3339() -> String {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, d.subsec_millis())
}

fn emails_to(db: &Db, to: &str) -> Vec<Email> {
    let mut v: Vec<Email> = db.emails.iter().filter(|e| to.is_empty() || e.to.eq_ignore_ascii_case(to)).cloned().collect();
    v.reverse(); // newest first, as the real APIs list them
    v
}

fn postmark_ok(h: &HeaderMap) -> bool {
    h.get("x-postmark-server-token").and_then(|v| v.to_str().ok()) == Some(POSTMARK_TOKEN)
}

// ------------------------------------------------------------------ GitHub Actions OIDC stand-in
//
// A test issuer shaped like token.actions.githubusercontent.com, so the suite
// can check how `enocone serve` treats CI tokens. The key is a fixed TEST key,
// committed on purpose; nothing trusts it except a hub pointed at this fixture.

/// The bearer token `GET /oidc/token` wants, like ACTIONS_ID_TOKEN_REQUEST_TOKEN.
pub const OIDC_REQUEST_TOKEN: &str = "fixture-oidc-request";
const OIDC_KEY_PEM: &str = include_str!("../assets/oidc-test-key.pem");
const OIDC_KEY_N: &str = include_str!("../assets/oidc-test-key.n");
const OIDC_KID: &str = "fixture-key-1";

fn issuer(h: &HeaderMap) -> String {
    let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("127.0.0.1:4010");
    format!("http://{host}/oidc")
}

async fn oidc_config(h: HeaderMap) -> Response {
    let iss = issuer(&h);
    Json(json!({ "issuer": iss, "jwks_uri": format!("{iss}/.well-known/jwks"), "id_token_signing_alg_values_supported": ["RS256"] })).into_response()
}

async fn oidc_jwks() -> Response {
    Json(json!({ "keys": [{ "kty": "RSA", "alg": "RS256", "use": "sig", "kid": OIDC_KID, "n": OIDC_KEY_N.trim(), "e": "AQAB" }] })).into_response()
}

/// Mint a token like GitHub's: `?audience=…&repository=owner/name&event_name=push&ref=refs/heads/main`.
async fn oidc_token(h: HeaderMap, Query(q): Query<HashMap<String, String>>) -> Response {
    if h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) != Some(&format!("bearer {OIDC_REQUEST_TOKEN}"))
        && h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) != Some(&format!("Bearer {OIDC_REQUEST_TOKEN}"))
    {
        return unauthorised();
    }
    let get = |k: &str, d: &str| q.get(k).cloned().unwrap_or_else(|| d.to_string());
    let repo = get("repository", "acme/shop");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let claims = json!({
        "iss": issuer(&h), "aud": q.get("aud_override").cloned().unwrap_or_else(|| get("audience", "")), "sub": format!("repo:{repo}:ref:{}", get("ref", "refs/heads/main")),
        "repository": repo, "repository_owner": repo.split('/').next().unwrap_or(""), "event_name": get("event_name", "push"),
        "ref": get("ref", "refs/heads/main"), "sha": "0123456789abcdef0123456789abcdef01234567", "actor": get("actor", "dev"),
        "workflow": "e2e", "workflow_ref": format!("{repo}/.github/workflows/{}@{}", get("workflow", "e2e.yml"), get("ref", "refs/heads/main")), "run_id": "1", "iat": now, "nbf": now, "exp": now + get("ttl", "300").parse::<u64>().unwrap_or(300),
    });
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(OIDC_KID.into());
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(OIDC_KEY_PEM.as_bytes()).expect("fixture test key");
    match jsonwebtoken::encode(&header, &claims, &key) {
        Ok(t) => Json(json!({ "count": 1, "value": t })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// ------------------------------------------------------------------ GitHub OAuth stand-in
//
// "Continue with GitHub" against a page where the test types who it is:
// a login, its teams (org/team, comma separated) and whether its email is
// verified. The access token is just the encoded identity.

fn form_page(title: &str, body: &str) -> Html<String> {
    page(title, body)
}

async fn gh_authorize_page(Query(q): Query<HashMap<String, String>>) -> Html<String> {
    let v = |k: &str| q.get(k).map(|s| s.replace('"', "&quot;").replace('<', "&lt;")).unwrap_or_default();
    form_page(
        "Sign in to GitHub",
        &format!(
            r#"<h1>Sign in to GitHub to continue to the app</h1><form method="post"><input type="hidden" name="redirect_uri" value="{}"><input type="hidden" name="state" value="{}"><input type="hidden" name="scope" value="{}">
<label>GitHub login <input name="login" required></label>
<label>Teams <input name="teams"></label>
<label>Email verified <input type="checkbox" name="verified" value="yes" checked></label>
<button type="submit">Authorize</button></form><p>Scopes requested: {}</p>"#,
            v("redirect_uri"),
            v("state"),
            v("scope"),
            v("scope")
        ),
    )
}

async fn gh_authorize(Form(f): Form<HashMap<String, String>>) -> Response {
    let get = |k: &str| f.get(k).cloned().unwrap_or_default();
    let ident = json!({ "login": get("login"), "teams": get("teams"), "verified": get("verified") == "yes" });
    let code = base64_url(&ident.to_string());
    let redirect = get("redirect_uri");
    let sep = if redirect.contains('?') { '&' } else { '?' };
    Redirect::to(&format!("{redirect}{sep}code={code}&state={}", get("state"))).into_response()
}

fn base64_url(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

fn ident_from(token: &str) -> Option<Value> {
    let hex = token.strip_prefix("gho_")?;
    let bytes: Option<Vec<u8>> = (0..hex.len()).step_by(2).map(|i| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok())).collect();
    serde_json::from_slice(&bytes?).ok()
}

async fn gh_access_token(Form(f): Form<HashMap<String, String>>) -> Response {
    if f.get("client_secret").map(String::as_str) != Some("fixture-github-secret") {
        return Json(json!({ "error": "incorrect_client_credentials" })).into_response();
    }
    Json(json!({ "access_token": format!("gho_{}", f.get("code").cloned().unwrap_or_default()), "token_type": "bearer", "scope": "read:user,user:email,read:org" })).into_response()
}

fn gh_ident(h: &HeaderMap) -> Option<Value> {
    let t = h.get(header::AUTHORIZATION)?.to_str().ok()?;
    ident_from(t.strip_prefix("Bearer ").or_else(|| t.strip_prefix("token "))?)
}

async fn gh_user(h: HeaderMap) -> Response {
    let Some(i) = gh_ident(&h) else { return unauthorised() };
    let login = i["login"].as_str().unwrap_or("");
    let id: u64 = login.bytes().fold(7u64, |a, b| a.wrapping_mul(31).wrapping_add(b as u64)) % 1_000_000_000;
    Json(json!({ "login": login, "id": id, "name": login })).into_response()
}

async fn gh_emails(h: HeaderMap) -> Response {
    let Some(i) = gh_ident(&h) else { return unauthorised() };
    Json(json!([{ "email": format!("{}@example.test", i["login"].as_str().unwrap_or("")), "primary": true, "verified": i["verified"].as_bool().unwrap_or(false) }])).into_response()
}

async fn gh_teams(h: HeaderMap) -> Response {
    let Some(i) = gh_ident(&h) else { return unauthorised() };
    let teams: Vec<Value> =
        i["teams"].as_str().unwrap_or("").split(',').map(str::trim).filter_map(|t| t.split_once('/')).map(|(org, slug)| json!({ "slug": slug, "organization": { "login": org } })).collect();
    Json(Value::Array(teams)).into_response()
}

/// Postmark's send API (`POST /email`): what enocone serve uses for sign-in links.
async fn postmark_send(State(s): State<S>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if !postmark_ok(&h) {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "ErrorCode": 10, "Message": "Bad or missing Server API token." }))).into_response();
    }
    let field = |k: &str| b.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let to = field("To");
    if to.is_empty() || field("From").is_empty() {
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "ErrorCode": 300, "Message": "Invalid email request" }))).into_response();
    }
    let e = Email { id: id("pm"), to: to.clone(), from: field("From"), subject: field("Subject"), text: field("TextBody"), html: field("HtmlBody"), sent_at: now_rfc3339() };
    let mid = e.id.clone();
    s.lock().unwrap().emails.push(e);
    Json(json!({ "To": to, "SubmittedAt": now_rfc3339(), "MessageID": mid, "ErrorCode": 0, "Message": "OK" })).into_response()
}

async fn postmark_list(State(s): State<S>, h: HeaderMap, Query(q): Query<HashMap<String, String>>) -> Response {
    if !postmark_ok(&h) {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "ErrorCode": 10, "Message": "No Account or Server API tokens were supplied in the HTTP headers." }))).into_response();
    }
    let list = emails_to(&s.lock().unwrap(), q.get("recipient").map(String::as_str).unwrap_or(""));
    let messages: Vec<Value> = list
        .iter()
        .map(|e| json!({ "MessageID": e.id, "MessageStream": "outbound", "To": [{ "Email": e.to, "Name": null }], "Recipients": [e.to], "ReceivedAt": e.sent_at, "From": e.from, "Subject": e.subject, "Status": "Sent", "Sandboxed": true }))
        .collect();
    Json(json!({ "TotalCount": messages.len(), "Messages": messages })).into_response()
}

async fn postmark_details(State(s): State<S>, h: HeaderMap, Path(id): Path<String>) -> Response {
    if !postmark_ok(&h) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match s.lock().unwrap().emails.iter().find(|e| e.id == id) {
        Some(e) => {
            Json(json!({ "MessageID": e.id, "To": [{ "Email": e.to }], "From": e.from, "Subject": e.subject, "TextBody": e.text, "HtmlBody": e.html, "ReceivedAt": e.sent_at, "Status": "Sent" }))
                .into_response()
        }
        None => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "ErrorCode": 701, "Message": "This message was not found." }))).into_response(),
    }
}

async fn mailpit_search(State(s): State<S>, Query(q): Query<HashMap<String, String>>) -> Response {
    // Only `to:"address"` (or a bare address) is understood.
    let query = q.get("query").cloned().unwrap_or_default();
    let to = query.trim().trim_start_matches("to:").trim_matches('"').to_string();
    let list = emails_to(&s.lock().unwrap(), &to);
    let messages: Vec<Value> = list
        .iter()
        .map(|e| json!({ "ID": e.id, "From": { "Address": e.from, "Name": "" }, "To": [{ "Address": e.to, "Name": "" }], "Subject": e.subject, "Created": e.sent_at, "Snippet": e.text.chars().take(80).collect::<String>() }))
        .collect();
    Json(json!({ "total": messages.len(), "messages_count": messages.len(), "messages": messages })).into_response()
}

async fn mailpit_message(State(s): State<S>, Path(id): Path<String>) -> Response {
    match s.lock().unwrap().emails.iter().find(|e| e.id == id) {
        Some(e) => Json(json!({ "ID": e.id, "From": { "Address": e.from }, "To": [{ "Address": e.to }], "Subject": e.subject, "Date": e.sent_at, "Text": e.text, "HTML": e.html })).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

// ---------------------------------------------------------------- short-lived tokens

/// Like `/api/auth/session`, but every call issues a new token that lasts 2 seconds.
async fn short_session(State(s): State<S>, h: HeaderMap) -> Json<Value> {
    if !signed_in(&h) {
        return Json(json!({}));
    }
    let mut db = s.lock().unwrap();
    let serial = db.short_tokens.len() as u64 + 1;
    let token = format!("st-{}", uuid::Uuid::new_v4().simple());
    db.short_tokens.insert(token.clone(), (serial, std::time::Instant::now()));
    Json(json!({ "user": { "name": USER }, "accessToken": token, "expiresIn": SHORT_TOKEN_TTL.as_secs() }))
}

/// Issues one short-lived token, then behaves as if the sign-in session had
/// expired (no token), so tests can see what a failed refresh looks like.
async fn short_session_once(State(s): State<S>, h: HeaderMap) -> Json<Value> {
    let first = s.lock().unwrap().short_tokens.is_empty();
    if first { short_session(State(s), h).await } else { Json(json!({ "user": null })) }
}

/// Which token was used (its serial number), if it is still fresh.
async fn short_whoami(State(s): State<S>, h: HeaderMap) -> Response {
    let token = h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("");
    match s.lock().unwrap().short_tokens.get(token) {
        Some((serial, at)) if at.elapsed() <= SHORT_TOKEN_TTL => Json(json!({ "user": USER, "token_serial": serial })).into_response(),
        Some(_) => (StatusCode::UNAUTHORIZED, Json(json!({ "error": "token expired" }))).into_response(),
        None => unauthorised(),
    }
}

// ------------------------------------------------------------- tables of examples

/// New customers get 5% off orders of 100 or more; returning customers 5%,
/// or 10% from 100. For enocone's own tests of tables of examples.
async fn discount(Json(b): Json<Value>) -> Json<Value> {
    let returning = b["customer"].as_str() == Some("returning");
    let total = b["total"].as_f64().unwrap_or(0.0);
    let pct = match (returning, total >= 100.0) {
        (false, false) => 0,
        (false, true) | (true, false) => 5,
        (true, true) => 10,
    };
    Json(json!({ "discount": format!("{pct}%") }))
}

/// Who may read the reports: admins (200), members are refused (403), anyone else isn't known (404).
async fn report_access(Path(who): Path<String>) -> Response {
    match who.as_str() {
        "admin" => Json(json!({ "reports": 3 })).into_response(),
        "member" => (StatusCode::FORBIDDEN, Json(json!({ "error": "not allowed" }))).into_response(),
        _ => (StatusCode::NOT_FOUND, Json(json!({ "error": "no such user" }))).into_response(),
    }
}

/// Three people and their invite status; the same for every group.
async fn people(Path(_group): Path<String>) -> Json<Value> {
    Json(json!([
        { "name": "ana", "status": "Pending" },
        { "name": "ben", "status": "Active" },
        { "name": "cy", "status": "Pending" }
    ]))
}

async fn invite(State(s): State<S>, Path(group): Path<String>, Json(b): Json<Value>) -> Response {
    let who = b["name"].as_str().unwrap_or("").to_string();
    s.lock().unwrap().invites.push((group, who.clone()));
    (StatusCode::CREATED, Json(json!({ "invited": who }))).into_response()
}

async fn invites(State(s): State<S>, Path(group): Path<String>) -> Json<Value> {
    let names: Vec<String> = s.lock().unwrap().invites.iter().filter(|(g, _)| *g == group).map(|(_, n)| n.clone()).collect();
    Json(json!({ "invited": names }))
}

async fn create_resource(State(s): State<S>, Path(group): Path<String>, Json(b): Json<Value>) -> Response {
    let rid = id("res");
    let r = json!({ "id": rid, "name": b["name"], "group": group });
    s.lock().unwrap().resources.insert(rid, r.clone());
    (StatusCode::CREATED, Json(r)).into_response()
}

async fn list_resources(State(s): State<S>, Path(group): Path<String>) -> Json<Value> {
    let mut list: Vec<Value> = s.lock().unwrap().resources.values().filter(|r| r["group"] == group).cloned().collect();
    list.sort_by_key(|r| r["id"].as_str().unwrap_or("").to_string());
    Json(json!({ "results": list }))
}

async fn delete_resource(State(s): State<S>, Path((group, rid)): Path<(String, String)>) -> StatusCode {
    let mut db = s.lock().unwrap();
    match db.resources.get(&rid) {
        Some(r) if r["group"] == group => {
            db.resources.remove(&rid);
            StatusCode::NO_CONTENT
        }
        _ => StatusCode::NOT_FOUND,
    }
}

/// The order's total: the sum of quantity × price over its items.
async fn order_total(Json(b): Json<Value>) -> Json<Value> {
    let items = b["items"].as_array().cloned().unwrap_or_default();
    let total: f64 = items.iter().map(|i| i["qty"].as_f64().unwrap_or(0.0) * i["price"].as_f64().unwrap_or(0.0)).sum();
    Json(json!({ "customer": b["customer"], "items": items.len(), "total": (total * 100.0).round() / 100.0 }))
}
