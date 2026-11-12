//! Playwright driver: cross-browser, auto-waiting, maintained upstream.
//!
//! `playwright-rs` speaks JSON-RPC to Microsoft's Playwright server, so role
//! and label locators, actionability checks and browser support come from
//! Playwright itself.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, anyhow};
use async_trait::async_trait;
use enocone_engine::driver::{Driver, NetRequest, Session, SessionOptions, UiAction, UiQuery};
use enocone_spec::blocks::{By, Locator};
use playwright_rs::protocol::{BrowserContext, BrowserContextOptions, GotoOptions, Locator as PwLocator, Page, StorageState, Viewport};
use playwright_rs::server::channel_owner::ChannelOwner;
use playwright_rs::{LaunchOptions, Playwright};
use serde_json::Value;

pub struct PlaywrightDriver {
    _pw: Playwright,
    browser: playwright_rs::protocol::Browser,
    name: String,
}

#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// chromium, firefox or webkit.
    pub browser: String,
    pub headless: bool,
    pub slow_mo_ms: Option<f64>,
    /// e.g. "chrome" to use an installed Google Chrome.
    pub channel: Option<String>,
}

impl PlaywrightDriver {
    pub async fn launch(cfg: &LaunchConfig) -> anyhow::Result<Self> {
        let pw = Playwright::launch().await.context("starting the Playwright server")?;
        let bt = match cfg.browser.as_str() {
            "chromium" | "chrome" => pw.chromium(),
            "firefox" => pw.firefox(),
            "webkit" | "safari" => pw.webkit(),
            other => return Err(anyhow!("unknown browser `{other}` (chromium, firefox, webkit)")),
        };
        let mut opts = LaunchOptions::new().headless(cfg.headless);
        if let Some(ms) = cfg.slow_mo_ms {
            opts = opts.slow_mo(ms);
        }
        if let Some(c) = &cfg.channel {
            opts = opts.channel(c.clone());
        }
        let browser = bt.launch_with_options(opts).await.map_err(|e| anyhow!("launching {}: {e}\n  hint: install browsers with `enocone install-browsers {}`", cfg.browser, cfg.browser))?;
        Ok(PlaywrightDriver { _pw: pw, browser, name: format!("playwright ({})", cfg.browser) })
    }
}

#[async_trait]
impl Driver for PlaywrightDriver {
    fn name(&self) -> String {
        self.name.clone()
    }

    async fn new_session(&self, opts: SessionOptions) -> anyhow::Result<Box<dyn Session>> {
        let mut b = BrowserContextOptions::builder().viewport(Viewport { width: 1280, height: 800 });
        if let Some(s) = opts.storage_state {
            let state: StorageState = serde_json::from_value(s).context("reading saved sign-in state")?;
            b = b.storage_state(state);
        }
        let ctx = self.browser.new_context_with_options(b.build()).await?;
        let page = ctx.new_page().await?;
        let console = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&console);
        page.on_console(move |msg| {
            let sink = Arc::clone(&sink);
            async move {
                let kind = msg.type_().to_string();
                if kind == "error" || kind == "warning" {
                    sink.lock().unwrap().push(format!("{kind}: {}", msg.text()));
                }
                Ok(())
            }
        })
        .await?;
        let requests: Requests = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&requests);
        ctx.on_request(move |r| {
            let sink = Arc::clone(&sink);
            async move {
                sink.lock().unwrap().push((r.guid().to_string(), NetRequest { method: r.method().to_string(), url: r.url().to_string(), status: None, done: false }));
                Ok(())
            }
        })
        .await?;
        for failed in [false, true] {
            let sink = Arc::clone(&requests);
            let done = move |r: playwright_rs::protocol::Request| {
                let sink = Arc::clone(&sink);
                async move {
                    let status = if failed { None } else { r.existing_response().map(|resp| resp.status()) };
                    if let Some((_, n)) = sink.lock().unwrap().iter_mut().find(|(g, _)| g == r.guid()) {
                        n.status = status;
                        n.done = true;
                    }
                    Ok(())
                }
            };
            if failed {
                ctx.on_request_failed(done).await?;
            } else {
                ctx.on_request_finished(done).await?;
            }
        }
        Ok(Box::new(PwSession { ctx, page, console, requests, timeout_ms: Mutex::new(0.0) }))
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        self.browser.close().await?;
        Ok(())
    }
}

/// Requests seen, keyed by Playwright's id for each.
type Requests = Arc<Mutex<Vec<(String, NetRequest)>>>;

struct PwSession {
    ctx: BrowserContext,
    page: Page,
    console: Arc<Mutex<Vec<String>>>,
    requests: Requests,
    timeout_ms: Mutex<f64>,
}

fn q(text: &str, exact: bool) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"{}", if exact { "s" } else { "i" })
}

fn json_q(text: &str, exact: bool) -> String {
    format!("{}{}", serde_json::to_string(text).unwrap(), if exact { "s" } else { "i" })
}

/// The Playwright selector for one `By` (without scope).
pub fn selector(by: &By) -> Option<String> {
    Some(match by {
        By::Role { role, name, exact } => match name {
            Some(n) => format!("internal:role={role}[name={}]", q(n, *exact)),
            None => format!("internal:role={role}"),
        },
        By::Label { text, exact } => format!("internal:label={}", json_q(text, *exact)),
        By::Text { text, exact } => format!("internal:text={}", json_q(text, *exact)),
        By::Placeholder(p) => format!("internal:attr=[placeholder={}]", json_q(p, false)),
        By::TestId(t) => format!("internal:testid=[data-testid={}]", json_q(t, true)),
        By::Css(c) => c.clone(),
        By::Clickable(_) | By::Page => return None,
    })
}

impl PwSession {
    fn build(&self, loc: &Locator) -> PwLocator {
        let scope: Option<PwLocator> = loc.within.as_ref().map(|w| self.build(w));
        let child = |sel: String| match &scope {
            Some(s) => s.locator(sel),
            None => self.page.locator(sel),
        };
        let mut l = match &loc.by {
            By::Page => scope.clone().unwrap_or_else(|| self.page.locator("html")),
            By::Clickable(name) => {
                let button = child(format!("internal:role=button[name={}]", q(name, false)));
                let link = child(format!("internal:role=link[name={}]", q(name, false)));
                button.or_(&link).first()
            }
            other => child(selector(other).expect("selector")),
        };
        if let Some(n) = loc.nth {
            l = if n < 0 { l.last() } else { l.nth(n) };
        }
        l
    }

    async fn set_timeout(&self, t: Duration) {
        let ms = t.as_millis() as f64;
        let changed = {
            let mut cur = self.timeout_ms.lock().unwrap();
            let c = *cur != ms;
            *cur = ms;
            c
        };
        if changed {
            self.page.set_default_timeout(ms).await;
        }
    }
}

fn clean(e: playwright_rs::Error) -> anyhow::Error {
    let msg = e.to_string();
    // Keep the first lines: Playwright's call logs are long.
    let short: Vec<&str> = msg.lines().take(6).collect();
    anyhow!(short.join("\n"))
}

#[async_trait]
impl Session for PwSession {
    async fn goto(&self, url: &str, timeout: Duration) -> anyhow::Result<()> {
        self.page.goto(url, GotoOptions::new().timeout(timeout)).await.map_err(clean)?;
        Ok(())
    }

    async fn act(&self, target: &Locator, action: UiAction, timeout: Duration) -> anyhow::Result<()> {
        self.set_timeout(timeout).await;
        let l = self.build(target);
        match action {
            UiAction::Click => l.click(None).await,
            UiAction::DblClick => l.dblclick(None).await,
            UiAction::Hover => l.hover(None).await,
            UiAction::Fill(v) => l.fill(&v, None).await,
            UiAction::Check => l.check(None).await,
            UiAction::Uncheck => l.uncheck(None).await,
            UiAction::Select(v) => {
                // Like Playwright's own select: the visible label or the value.
                let labels = l.locator("option").all_inner_texts().await.unwrap_or_default();
                let by = if labels.iter().any(|t| t.trim() == v) { playwright_rs::protocol::SelectOption::Label(v.clone()) } else { playwright_rs::protocol::SelectOption::Value(v.clone()) };
                l.select_option(by, None).await.map(|_| ())
            }
            UiAction::Press(k) => l.press(&k, None).await,
            UiAction::Upload(f) => l.set_input_files(&std::path::PathBuf::from(f), None).await,
        }
        .map_err(clean)
    }

    async fn press(&self, key: &str) -> anyhow::Result<()> {
        self.page.keyboard().press(key, None).await.map_err(clean)
    }

    async fn query(&self, target: &Locator, query: UiQuery) -> anyhow::Result<Value> {
        let l = self.build(target);
        let count = l.count().await.map_err(clean)?;
        if let UiQuery::Count = query {
            return Ok(Value::from(count));
        }
        if count == 0 {
            return Ok(match query {
                UiQuery::Visible => Value::Bool(false),
                _ => Value::Null,
            });
        }
        let first = if target.nth.is_some() { l.clone() } else { l.first() };
        Ok(match query {
            UiQuery::Visible => {
                // Any of the first few matches being visible counts.
                for i in 0..count.min(5) {
                    let item = if target.nth.is_some() { l.clone() } else { l.nth(i as i32) };
                    if item.is_visible().await.unwrap_or(false) {
                        return Ok(Value::Bool(true));
                    }
                }
                Value::Bool(false)
            }
            UiQuery::Text => Value::String(first.inner_text().await.map_err(clean)?),
            UiQuery::Value => Value::String(first.input_value(None).await.map_err(clean)?),
            UiQuery::Checked => Value::Bool(first.is_checked().await.map_err(clean)?),
            UiQuery::Enabled => Value::Bool(first.is_enabled().await.map_err(clean)?),
            UiQuery::Attribute(a) => first.get_attribute(&a).await.map_err(clean)?.map(Value::String).unwrap_or(Value::Null),
            UiQuery::Count => unreachable!(),
        })
    }

    async fn url(&self) -> anyhow::Result<String> {
        Ok(self.page.url())
    }

    async fn title(&self) -> anyhow::Result<String> {
        self.page.title().await.map_err(clean)
    }

    async fn snapshot(&self) -> anyhow::Result<String> {
        self.page.locator("body").aria_snapshot(None).await.map_err(clean)
    }

    async fn screenshot(&self) -> anyhow::Result<Vec<u8>> {
        self.page.screenshot(None).await.map_err(clean)
    }

    async fn eval(&self, js: &str) -> anyhow::Result<Value> {
        self.page.evaluate::<(), Value>(js, None).await.map_err(clean)
    }

    async fn storage_state(&self) -> anyhow::Result<Value> {
        let s = self.ctx.storage_state(None).await.map_err(clean)?;
        Ok(serde_json::to_value(s)?)
    }

    async fn take_console(&self) -> Vec<String> {
        std::mem::take(&mut *self.console.lock().unwrap())
    }

    async fn requests(&self) -> Vec<NetRequest> {
        self.requests.lock().unwrap().iter().map(|(_, n)| n.clone()).collect()
    }

    async fn clear_requests(&self) {
        self.requests.lock().unwrap().clear();
    }

    async fn close(&self) -> anyhow::Result<()> {
        self.ctx.close().await.map_err(clean)
    }
}

/// Install the browser builds matching the bundled Playwright driver.
/// Where the Playwright driver comes from, as `enocone doctor` reports it.
static DRIVER_SOURCE: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// The driver folder shipped beside the binary in release archives (and in
/// the Homebrew keg): `<dir of the real executable>/playwright-driver`.
pub fn bundled_driver_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // Homebrew links bin/enocone to the keg; follow it to where the driver is.
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let dir = exe.parent()?.join("playwright-driver");
    dir.join("package").join("cli.js").is_file().then_some(dir)
}

/// Point Playwright at the driver beside the binary, unless the user chose one
/// with `PLAYWRIGHT_DRIVER_PATH`. Call first thing in `main`, before any
/// threads exist: it sets an environment variable.
pub fn use_bundled_driver() {
    if std::env::var_os("PLAYWRIGHT_DRIVER_PATH").is_some_and(|v| !v.is_empty()) {
        let _ = DRIVER_SOURCE.set("PLAYWRIGHT_DRIVER_PATH");
        return;
    }
    match bundled_driver_dir() {
        Some(dir) => {
            // SAFETY: called from main before the async runtime or any other thread starts.
            unsafe { std::env::set_var("PLAYWRIGHT_DRIVER_PATH", &dir) };
            let _ = DRIVER_SOURCE.set("bundled with enocone");
        }
        None => {
            let _ = DRIVER_SOURCE.set("assembled when enocone was built, or the user cache");
        }
    }
}

/// Which driver Playwright will use: (where it came from, the CLI script it runs).
pub fn driver_info() -> (String, Result<std::path::PathBuf, String>) {
    let source = DRIVER_SOURCE.get().copied().unwrap_or("not resolved yet").to_string();
    let path = playwright_rs::server::driver::get_driver_executable().map(|(_, cli)| cli).map_err(|e| e.to_string());
    (source, path)
}

pub async fn install_browsers(names: &[String], with_deps: bool) -> anyhow::Result<()> {
    let sel: Vec<&str> = names.iter().map(String::as_str).collect();
    let sel = (!sel.is_empty()).then_some(sel.as_slice());
    if with_deps {
        playwright_rs::install_browsers_with_deps(sel).await?;
    } else {
        playwright_rs::install_browsers(sel).await?;
    }
    Ok(())
}
