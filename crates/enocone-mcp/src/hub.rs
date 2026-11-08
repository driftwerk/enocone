//! `enocone login` and `enocone publish`: sending runs to a team site.
//!
//! Tokens are kept in the system keychain on macOS and Windows, otherwise in
//! `~/.config/enocone/credentials` (readable only by you). `ENOCONE_HUB_TOKEN`
//! overrides both, for CI; `ENOCONE_CREDENTIALS_FILE` forces the file store.

use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, Read as _, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use base64::Engine as _;
use enocone_spec::RunResults;
use serde_json::{Value, json};

/// Open a URL in the default browser.
pub fn open_url(u: &str) {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd).arg(u).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

pub const DEFAULT_HUB: &str = "https://enoc.one";

pub fn normalise_url(u: &str) -> String {
    u.trim().trim_end_matches('/').to_string()
}

// ------------------------------------------------------------------ credentials

fn credentials_file() -> PathBuf {
    if let Some(p) = std::env::var_os("ENOCONE_CREDENTIALS_FILE").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("enocone").join("credentials")
}

fn use_file_store() -> bool {
    std::env::var_os("ENOCONE_CREDENTIALS_FILE").is_some_and(|p| !p.is_empty()) || !cfg!(any(target_os = "macos", target_os = "windows"))
}

fn read_file_store() -> BTreeMap<String, String> {
    std::fs::read_to_string(credentials_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn write_file_store(m: &BTreeMap<String, String>) -> anyhow::Result<PathBuf> {
    let path = credentials_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut f = o.open(&tmp)?;
        f.write_all(serde_json::to_string_pretty(m)?.as_bytes())?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

/// Where the token went, for the message.
fn store_token(url: &str, token: &str) -> anyhow::Result<String> {
    if use_file_store() {
        let mut m = read_file_store();
        m.insert(url.to_string(), token.to_string());
        let p = write_file_store(&m)?;
        return Ok(p.display().to_string());
    }
    keychain_set(url, token)?;
    Ok("the system keychain".into())
}

fn stored_token(url: &str) -> Option<String> {
    if use_file_store() {
        return read_file_store().get(url).cloned();
    }
    keychain_get(url)
}

fn forget_token(url: &str) -> anyhow::Result<()> {
    if use_file_store() {
        let mut m = read_file_store();
        if m.remove(url).is_some() {
            write_file_store(&m)?;
        }
        return Ok(());
    }
    keychain_delete(url)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn keychain_set(url: &str, token: &str) -> anyhow::Result<()> {
    keyring::Entry::new("enocone", url)?.set_password(token).context("saving the token in the keychain")
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn keychain_get(url: &str) -> Option<String> {
    keyring::Entry::new("enocone", url).ok()?.get_password().ok()
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn keychain_delete(url: &str) -> anyhow::Result<()> {
    match keyring::Entry::new("enocone", url)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn keychain_set(_: &str, _: &str) -> anyhow::Result<()> {
    unreachable!("the file store is used on this platform")
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn keychain_get(_: &str) -> Option<String> {
    None
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn keychain_delete(_: &str) -> anyhow::Result<()> {
    Ok(())
}

/// The token for `url`: ENOCONE_HUB_TOKEN; in GitHub Actions with `id-token: write`,
/// a fresh OIDC token for that site; otherwise the one `enocone login` saved.
pub async fn token_for(url: &str) -> anyhow::Result<Option<String>> {
    if let Some(t) = std::env::var("ENOCONE_HUB_TOKEN").ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        return Ok(Some(t));
    }
    if let (Ok(req), Ok(bearer)) = (std::env::var("ACTIONS_ID_TOKEN_REQUEST_URL"), std::env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN")) {
        let sep = if req.contains('?') { '&' } else { '?' };
        let res = client().get(format!("{req}{sep}audience={}", enocode(url))).header("Authorization", format!("bearer {bearer}")).send().await.context("asking GitHub Actions for an OIDC token")?;
        if !res.status().is_success() {
            bail!("GitHub Actions wouldn't give an OIDC token ({}); the job needs `permissions: id-token: write`", error_text(res).await);
        }
        let v: Value = res.json().await?;
        return Ok(v["value"].as_str().map(String::from));
    }
    Ok(stored_token(url))
}

// ------------------------------------------------------------------ API

fn client() -> reqwest::Client {
    reqwest::Client::builder().user_agent(concat!("enocone/", env!("CARGO_PKG_VERSION"))).build().expect("http client")
}

async fn error_text(res: reqwest::Response) -> String {
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<Value>(&body).ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or(body);
    format!("{status}: {msg}")
}

/// The open review comments on a flow (all of them with `all`).
/// A flow's review comments. Without a token, only a public project answers.
pub async fn comments(url: &str, token: Option<&str>, project: &str, flow_id: &str, all: bool) -> anyhow::Result<Value> {
    let mut req = client().get(format!("{url}/api/projects/{}/flows/{}/comments{}", enocode(project), enocode(flow_id), if all { "?all=true" } else { "" }));
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let res = req.send().await.with_context(|| format!("reaching {url}"))?;
    if !res.status().is_success() {
        bail!("{}", error_text(res).await);
    }
    Ok(res.json().await?)
}

pub async fn whoami(url: &str, token: &str) -> anyhow::Result<Value> {
    let res = client().get(format!("{url}/api/whoami")).bearer_auth(token).send().await.with_context(|| format!("couldn't reach {url}"))?;
    if !res.status().is_success() {
        bail!("{url} refused the token ({})", error_text(res).await);
    }
    Ok(res.json().await?)
}

// ------------------------------------------------------------------ login

pub async fn login(url: &str, with_token: bool) -> anyhow::Result<()> {
    let token = if with_token {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        s.trim().to_string()
    } else {
        browser_login(url).await?
    };
    if token.is_empty() {
        bail!("no token given");
    }
    let me = whoami(url, &token).await?;
    let place = store_token(url, &token)?;
    let projects: Vec<&str> = me["projects"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    println!("signed in to {url} as {}", me["name"].as_str().unwrap_or("?"));
    eprintln!("token saved in {place}");
    if projects.is_empty() {
        eprintln!("you aren't on any project yet; ask a project owner to invite you");
    } else {
        eprintln!("projects: {}", projects.join(", "));
    }
    Ok(())
}

pub fn logout(url: &str) -> anyhow::Result<()> {
    forget_token(url)?;
    println!("signed out of {url} on this computer (revoke the token on the site to stop it everywhere)");
    Ok(())
}

/// Open the site in a browser; it hands a new token back to a one-off local port.
async fn browser_login(url: &str) -> anyhow::Result<String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let mut b = [0u8; 16];
    getrandom(&mut b);
    let state: String = b.iter().map(|x| format!("{x:02x}")).collect();
    let host = hostname();
    let link = format!("{url}/cli/authorize?port={port}&state={state}&name={}", enocode(&host));
    eprintln!("Opening {url} to sign in. If no browser opens, visit:\n  {link}");
    open_url(&link);
    let token = tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
        listener.set_nonblocking(false)?;
        loop {
            let (mut stream, _) = listener.accept()?;
            let mut line = String::new();
            std::io::BufReader::new(&stream).read_line(&mut line)?;
            let target = line.split_whitespace().nth(1).unwrap_or("");
            let q: BTreeMap<String, String> =
                target.split_once('?').map(|(_, q)| q.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect()).unwrap_or_default();
            let ok = q.get("state") == Some(&state) && q.get("token").is_some_and(|t| t.starts_with("enc_"));
            let page = if ok { "Signed in. You can close this tab and go back to the terminal." } else { "That didn't work. Run enocone login again." };
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\n\r\n{page}\n");
            if ok {
                return Ok(q["token"].clone());
            }
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(300), token).await.context("timed out waiting for the browser (5 minutes)")??
}

fn getrandom(b: &mut [u8]) {
    getrandom::fill(b).expect("the OS random number generator");
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::process::Command::new("hostname").output().ok().and_then(|o| String::from_utf8(o.stdout).ok()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "this computer".into())
}

fn enocode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

// ------------------------------------------------------------------ publish

/// Screenshots `results` refers to, read from `dir`.
fn screenshots(results: &RunResults, dir: &Path) -> anyhow::Result<BTreeMap<String, String>> {
    let mut paths = HashSet::new();
    fn walk(s: &enocone_spec::results::StepResult, out: &mut HashSet<String>) {
        if let Some(p) = &s.screenshot
            && !p.starts_with("data:")
        {
            out.insert(p.clone());
        }
        for c in &s.children {
            walk(&c.result, out);
        }
    }
    for f in &results.flows {
        for a in &f.attempts {
            for s in &a.steps {
                walk(s, &mut paths);
            }
        }
    }
    let mut files = BTreeMap::new();
    for p in paths {
        if let Ok(bytes) = std::fs::read(dir.join(&p)) {
            files.insert(p, base64::engine::general_purpose::STANDARD.encode(bytes));
        }
    }
    Ok(files)
}

pub async fn publish(url: &str, project: &str, results_path: &Path) -> anyhow::Result<String> {
    let text = std::fs::read_to_string(results_path).with_context(|| format!("reading {} (run enocone first)", results_path.display()))?;
    let results: RunResults = serde_json::from_str(&text).with_context(|| format!("{} isn't an enocone results file", results_path.display()))?;
    let dir = results_path.parent().unwrap_or(Path::new("."));
    let files = screenshots(&results, dir)?;
    let Some(token) = token_for(url).await? else {
        bail!("not signed in to {url}: run `enocone login`, or set ENOCONE_HUB_TOKEN");
    };
    let source = if std::env::var_os("GITHUB_ACTIONS").is_some() { "ci" } else { "cli" };
    let body = json!({ "results": serde_json::from_str::<Value>(&text)?, "files": files, "source": source });
    let n = files_len(&body);
    let res = client().post(format!("{url}/api/projects/{project}/runs")).bearer_auth(token).json(&body).send().await.with_context(|| format!("couldn't reach {url}"))?;
    if !res.status().is_success() {
        bail!("{url} refused the run ({})", error_text(res).await);
    }
    let v: Value = res.json().await?;
    let link = v["url"].as_str().unwrap_or(url).to_string();
    eprintln!("published {} flows and {n} screenshots", results.flows.len());
    Ok(link)
}

fn files_len(body: &Value) -> usize {
    body["files"].as_object().map(|m| m.len()).unwrap_or(0)
}
