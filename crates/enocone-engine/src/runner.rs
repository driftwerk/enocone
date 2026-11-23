//! Run flows: one isolated context per flow, steps in document order,
//! cleanup always, retries marked as flaky, evidence on every step.

use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use enocone_spec::blocks::{Block, UseSpec, parse_duration_str};
use enocone_spec::results::*;
use enocone_spec::template::{Resolver, render};
use enocone_spec::{BlockKind, Flow, SectionKind, Step};
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::auth::{AuthState, Need, Next, ProfileAuth, SIGN_IN_STEPS, outcome_of};
use crate::browser::{Timeouts, run_browser};
use crate::config::{AuthProfile, BrowserToken, Project, Screenshots};
use crate::driver::{Driver, Route, Session, SessionOptions};
use crate::email::{Mailbox, Provider, run_email};
use crate::exec::run_exec;
use crate::http::{json_at, run_http};
use crate::vars::{Masker, Vars};

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub env: String,
    pub run_id: String,
    pub out_dir: PathBuf,
    pub workers: usize,
    pub retries: u32,
    pub timeout: Duration,
    pub navigation_timeout: Duration,
    pub screenshots: Screenshots,
    pub insecure: bool,
    /// `[[run.routes]]`, given to every browser session.
    pub routes: Arc<[Route]>,
    /// Run against a prototype served at this origin: draft flows run, http and
    /// exec blocks are skipped, and `sample:` values stand in for their captures.
    pub prototype: Option<String>,
}

impl RunOptions {
    pub fn from_project(project: &Project, env: String, out_dir: PathBuf) -> anyhow::Result<Self> {
        let rc = &project.config.run;
        Ok(RunOptions {
            env,
            run_id: new_run_id(),
            out_dir,
            workers: rc.workers.max(1),
            retries: rc.retries,
            timeout: parse_duration_str(&rc.timeout).map_err(|e| anyhow!("run.timeout: {e}"))?,
            navigation_timeout: parse_duration_str(&rc.navigation_timeout).map_err(|e| anyhow!("run.navigation_timeout: {e}"))?,
            screenshots: rc.screenshots,
            insecure: rc.insecure,
            routes: Arc::clone(&project.routes),
            prototype: None,
        })
    }
}

pub fn new_run_id() -> String {
    format!("{}-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"), &uuid::Uuid::new_v4().simple().to_string()[..4])
}

/// Progress events for the CLI / MCP.
#[derive(Debug, Clone)]
pub enum Event {
    FlowStarted { id: String, title: String },
    FlowFinished(Box<FlowResult>),
    Retrying { id: String, attempt: u32 },
    Log(String),
}

pub type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

pub struct Engine {
    pub project: Project,
    pub opts: RunOptions,
    driver: Option<Arc<dyn Driver>>,
    http: reqwest::Client,
    pub masker: Masker,
    env_vars: serde_json::Map<String, Value>,
    auth: std::sync::Mutex<HashMap<String, Arc<ProfileAuth>>>,
    /// Which profile's login waits on which profile's attempt, now: the
    /// edges along which logins could wait for each other.
    waits: std::sync::Mutex<HashMap<String, String>>,
    events: EventSink,
}

struct FlowState {
    vars: Vars,
    sessions: HashMap<String, Box<dyn Session>>,
    flow_id: String,
    attempt: u32,
    /// Only emails sent after this count.
    started: chrono::DateTime<chrono::Utc>,
    /// The profiles whose logins these steps run inside, outermost first.
    signing_in: Vec<String>,
    /// How long closing each browser may take.
    close_timeout: Duration,
}

impl FlowState {
    async fn close_sessions(&mut self) {
        for (_, s) in self.sessions.drain() {
            close_within(s, self.close_timeout).await;
        }
    }
}

/// Browsers still open when a flow's state goes (a sign-in, or a use that
/// was cancelled) are closed in the background.
impl Drop for FlowState {
    fn drop(&mut self) {
        for (_, s) in self.sessions.drain() {
            close_in_background(s, self.close_timeout);
        }
    }
}

/// Close a browser session, giving up after `timeout` so a hung close holds nothing up.
async fn close_within(session: Box<dyn Session>, timeout: Duration) {
    let _ = tokio::time::timeout(timeout, session.close()).await;
}

/// Close a browser session in a task of its own, so nothing waits for it.
fn close_in_background(session: Box<dyn Session>, timeout: Duration) {
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        runtime.spawn(close_within(session, timeout));
    }
}

/// A browser session that is closed in the background when it is dropped,
/// on every path: done with, failed, timed out or cancelled.
struct ClosedOnDrop {
    session: Option<Box<dyn Session>>,
    timeout: Duration,
}

impl ClosedOnDrop {
    fn session(&self) -> anyhow::Result<&dyn Session> {
        self.session.as_deref().ok_or_else(|| anyhow!("the browser session is gone"))
    }

    /// The session, for a holder that closes it itself from now on.
    fn into_session(mut self) -> anyhow::Result<Box<dyn Session>> {
        self.session.take().ok_or_else(|| anyhow!("the browser session is gone"))
    }
}

/// Open a browser in a task of its own, so that one opening after its caller
/// gave up or was dropped is still closed, in the background, when it opens.
async fn open_browser(driver: &Arc<dyn Driver>, options: SessionOptions, close_timeout: Duration) -> anyhow::Result<ClosedOnDrop> {
    let driver = Arc::clone(driver);
    let (opened, opening) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let session = driver.new_session(options).await.map(|s| ClosedOnDrop { session: Some(s), timeout: close_timeout });
        // Sent or not, the session closes itself once nobody holds it.
        let _ = opened.send(session);
    });
    opening.await.map_err(|_| anyhow!("the task opening the browser ended without one"))?
}

/// While held, `from`'s login waits on `to`'s attempt.
struct Waiting<'a> {
    waits: &'a std::sync::Mutex<HashMap<String, String>>,
    from: String,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.waits.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&self.from);
    }
}

impl Drop for ClosedOnDrop {
    fn drop(&mut self) {
        if let Some(session) = self.session.take() {
            close_in_background(session, self.timeout);
        }
    }
}

/// Why a login can never finish: following `path`, it signs in as its own profile again.
fn login_loop(path: &[String]) -> String {
    format!("its login signs in as itself ({}), so it would wait for its own sign-in forever", path.join(" → "))
}

type StepFuture<'a> = Pin<Box<dyn Future<Output = StepResult> + Send + 'a>>;

impl Engine {
    pub fn new(project: Project, opts: RunOptions, driver: Option<Arc<dyn Driver>>, events: EventSink) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder().user_agent(concat!("enocone/", env!("CARGO_PKG_VERSION"))).danger_accept_invalid_certs(opts.insecure).build()?;
        let mut env_vars = project.env_vars(&opts.env);
        if let Some(origin) = &opts.prototype {
            // Every web address in the environment points at the prototype, keeping its path.
            for v in env_vars.values_mut() {
                if let Some(u) = v.as_str()
                    && let Ok(parsed) = reqwest::Url::parse(u)
                    && matches!(parsed.scheme(), "http" | "https")
                {
                    let mut tail = parsed.path().trim_end_matches('/').to_string();
                    if let Some(q) = parsed.query() {
                        tail.push('?');
                        tail.push_str(q);
                    }
                    *v = Value::String(format!("{}{tail}", origin.trim_end_matches('/')));
                }
            }
        }
        Ok(Engine { project, opts, driver, http, masker: Masker::default(), env_vars, auth: std::sync::Mutex::new(HashMap::new()), waits: std::sync::Mutex::new(HashMap::new()), events })
    }

    pub fn driver_name(&self) -> String {
        self.driver.as_ref().map(|d| d.name()).unwrap_or_else(|| "none (http only)".into())
    }

    /// The HTTP client the engine uses (shared settings: user agent, TLS).
    pub fn http_client(&self) -> &reqwest::Client {
        &self.http
    }

    /// Headers an auth profile adds to HTTP requests (signs in once per engine).
    pub async fn auth_headers(&self, profile: &str) -> Result<Vec<(String, String)>, String> {
        self.auth(profile, Need::Token, &[]).await.map(|a| a.headers)
    }

    /// A browser session for exploration, optionally signed in as a profile
    /// (its kept sign-in, so a failed token refresh doesn't stop it).
    pub async fn open_session(&self, profile: Option<&str>) -> anyhow::Result<Box<dyn Session>> {
        let driver = self.driver.as_ref().ok_or_else(|| anyhow!("no browser driver is running"))?;
        let storage = match profile.filter(|p| !p.is_empty()) {
            None => None,
            Some(p) => {
                let a = self.auth(p, Need::SignIn, &[]).await.map_err(|e| anyhow!("auth profile `{p}`: {e}"))?;
                Some(a.storage.ok_or_else(|| anyhow!("auth profile `{p}` is not a browser login"))?)
            }
        };
        open_browser(driver, SessionOptions { storage_state: storage, routes: Arc::clone(&self.opts.routes) }, self.opts.timeout).await?.into_session()
    }

    /// The project's `[email]` mailbox, with templates (secrets) filled in.
    fn mailbox(&self) -> Result<Mailbox, String> {
        let c = self.project.config.email.as_ref().ok_or("email blocks need a mailbox: add [email] to enocone.toml with provider = \"postmark\", \"mailpit\" or \"mailhog\"")?;
        let provider = Provider::parse(&c.provider)?;
        let url = match &c.url {
            Some(u) => self.render_str(u).map_err(|e| format!("[email] url: {e}"))?,
            None => provider.default_url().to_string(),
        };
        let token = c.token.as_ref().map(|t| self.render_str(t)).transpose().map_err(|e| format!("[email] token: {e}"))?;
        if let Some(t) = &token {
            self.masker.add(t);
        }
        Ok(Mailbox { provider, url, token })
    }

    /// Render `{{…}}` in a string against the env (no captured values).
    pub fn render_value(&self, v: &Value) -> anyhow::Result<Value> {
        render(v, &self.vars(0)).map_err(|e| anyhow!(e))
    }

    pub fn has_driver(&self) -> bool {
        self.driver.is_some()
    }

    pub fn base_url(&self) -> Option<String> {
        ["WEB", "web", "BASE_URL", "base_url"].iter().find_map(|k| self.env_vars.get(*k).and_then(Value::as_str).map(String::from))
    }

    fn vars(&self, attempt: u32) -> Vars {
        Vars::new(self.env_vars.clone(), &self.opts.run_id, attempt, self.masker.clone())
    }

    /// Run flows with up to `workers` in parallel. Results keep input order.
    pub async fn run(self: &Arc<Self>, flows: Vec<Flow>) -> RunResults {
        let started_at = chrono::Utc::now();
        let started = Instant::now();
        let sem = Arc::new(Semaphore::new(self.opts.workers));
        let mut handles = Vec::new();
        for flow in flows {
            let me = Arc::clone(self);
            let sem = Arc::clone(&sem);
            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire_owned().await?;
                (me.events)(Event::FlowStarted { id: flow.id.clone(), title: flow.title.clone() });
                let r = me.run_flow(flow).await;
                (me.events)(Event::FlowFinished(Box::new(r.clone())));
                anyhow::Ok(r)
            }));
        }
        let mut results = Vec::new();
        for h in handles {
            match h.await.map_err(anyhow::Error::from).and_then(|r| r) {
                Ok(r) => results.push(r),
                Err(e) => (self.events)(Event::Log(format!("internal error: {e}"))),
            }
        }
        self.masker.scrub(RunResults {
            schema: RESULTS_SCHEMA.into(),
            run: RunInfo {
                id: self.opts.run_id.clone(),
                project: self.project.config.project.clone(),
                env: self.opts.env.clone(),
                driver: self.driver_name(),
                started_at: started_at.to_rfc3339(),
                duration_ms: started.elapsed().as_millis() as u64,
                enocone_version: env!("CARGO_PKG_VERSION").into(),
                executed: true,
                prototype: self.opts.prototype.is_some(),
                ..Default::default()
            },
            summary: Summary::from_flows(&results),
            flows: results,
            theme: None,
        })
    }

    fn skip_reason(&self, flow: &Flow) -> Option<(Status, String)> {
        if flow.meta.skip {
            return Some((Status::Skipped, "marked skip: true".into()));
        }
        if !flow.meta.stage.runs() && self.opts.prototype.is_none() {
            return Some((Status::Pending, format!("{}: behaviour not approved yet, so it is not run", flow.meta.stage.as_str())));
        }
        if !flow.meta.envs.is_empty() && !flow.meta.envs.iter().any(|e| e == &self.opts.env) {
            return Some((Status::Skipped, format!("only runs on {}", flow.meta.envs.join(", "))));
        }
        let vars = self.vars(0);
        let missing: Vec<&String> = flow.meta.requires.iter().filter(|r| vars.lookup(r).is_none_or(|v| v.is_null() || v == "")).collect();
        if !missing.is_empty() {
            return Some((Status::Skipped, format!("requires {}", missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))));
        }
        None
    }

    pub async fn run_flow(&self, flow: Flow) -> FlowResult {
        let started = Instant::now();
        if let Some((status, why)) = self.skip_reason(&flow) {
            let steps = flow
                .steps()
                .map(|s| StepResult {
                    key: s.key.clone(),
                    status: if status == Status::Pending { Status::Pending } else { Status::Skipped },
                    duration_ms: 0,
                    error: None,
                    note: Some(why.clone()),
                    records: vec![],
                    captured: vec![],
                    screenshot: None,
                    snapshot: None,
                    page_url: None,
                    console: vec![],
                    children: vec![],
                    rows: vec![],
                })
                .collect();
            return FlowResult { flow, status, skip_reason: Some(why), duration_ms: 0, attempts: vec![Attempt { status: Some(status), steps, ..Default::default() }] };
        }
        let mut attempts = Vec::new();
        for n in 1..=self.opts.retries + 1 {
            if n > 1 {
                (self.events)(Event::Retrying { id: flow.id.clone(), attempt: n });
            }
            let a = self.attempt(&flow, n).await;
            let failed = a.status == Some(Status::Failed);
            attempts.push(a);
            if !failed {
                break;
            }
        }
        let last = attempts.last().and_then(|a| a.status).unwrap_or(Status::Failed);
        let status = if last == Status::Passed && attempts.len() > 1 { Status::Flaky } else { last };
        // Values captured as secret were recorded before they were known: mask them everywhere now.
        self.masker.scrub(FlowResult { flow, status, skip_reason: None, duration_ms: started.elapsed().as_millis() as u64, attempts })
    }

    /// Keep a block's captures for later steps; secret ones are masked from now on.
    fn keep(&self, st: &mut FlowState, r: &mut StepResult, captured: Vec<(String, Value)>, secret_names: &[String]) {
        for (k, v) in captured {
            if secret_names.contains(&k) || self.project.config.mask.captures.contains(&k) {
                match &v {
                    Value::String(s) => self.masker.add_capture(&k, s),
                    other => self.masker.add_capture(&k, &other.to_string()),
                }
            }
            r.captured.push((k.clone(), self.masker.mask(&value_preview(&v))));
            st.vars.set(&k, v);
        }
    }

    async fn attempt(&self, flow: &Flow, n: u32) -> Attempt {
        let started = Instant::now();
        let mut st =
            FlowState { vars: self.vars(n), sessions: HashMap::new(), flow_id: flow.id.clone(), attempt: n, started: chrono::Utc::now(), signing_in: vec![], close_timeout: self.opts.timeout };
        // A scratch folder of its own for this attempt: `{{run.tmp}}`.
        let tmp = std::env::temp_dir().join(format!("enocone-{}", self.opts.run_id)).join(format!("{}-{n}", sanitize(&flow.id)));
        if std::fs::create_dir_all(&tmp).is_ok() {
            st.vars.run.insert("tmp".into(), Value::String(tmp.to_string_lossy().to_string()));
        }
        if self.opts.prototype.is_some() {
            for (k, v) in &flow.sample {
                st.vars.set(k, v.clone());
            }
        }
        let mut steps = Vec::new();
        let mut blocked: Option<(Status, String)> = None;
        for section in flow.sections.iter().filter(|s| s.kind != SectionKind::Cleanup) {
            for step in &section.steps {
                let label = step.number.map(|n| format!("step {n}")).unwrap_or_else(|| "an earlier step".into());
                if let Some((status, why)) = &blocked {
                    steps.push(note_only(step, *status, why));
                    continue;
                }
                if step.blocks.is_empty() {
                    steps.push(note_only(step, Status::Pending, "not implemented yet: add an http, browser, expect, use, exec or email block"));
                    blocked = Some((Status::Pending, format!("waits for {label} to be implemented")));
                    continue;
                }
                let r = self.exec_step(&mut st, step, "").await;
                if r.status == Status::Failed {
                    blocked = Some((Status::Skipped, format!("not run because {label} failed")));
                }
                steps.push(r);
            }
        }
        for section in flow.sections.iter().filter(|s| s.kind == SectionKind::Cleanup) {
            for step in &section.steps {
                if step.blocks.is_empty() {
                    steps.push(note_only(step, Status::Pending, "not implemented yet"));
                } else {
                    steps.push(self.exec_step(&mut st, step, "").await);
                }
            }
        }
        st.close_sessions().await;
        let status = if steps.iter().any(|s| s.status == Status::Failed) {
            Status::Failed
        } else if steps.iter().any(|s| s.status == Status::Pending) {
            Status::Pending
        } else {
            Status::Passed
        };
        let captured = st.vars.captured.iter().map(|(k, v)| (k.clone(), self.masker.mask(&value_preview(v)))).collect();
        Attempt { status: Some(status), duration_ms: started.elapsed().as_millis() as u64, steps, captured, artifacts: vec![] }
    }

    /// Run a step: once, or once per row of its table of examples.
    fn exec_step<'a>(&'a self, st: &'a mut FlowState, step: &'a Step, prefix: &'a str) -> StepFuture<'a> {
        Box::pin(async move {
            let started = Instant::now();
            // The cases the step runs for: each row of its tables, or each item of its list.
            struct Case {
                var: &'static str,
                value: Value,
                row: usize,
                table: usize,
                table_name: Option<String>,
                label: String,
                line: usize,
                prefix: String,
            }
            let mut cases = Vec::new();
            if let Some(fe) = &step.for_each {
                let items = match st.vars.lookup(&fe.list) {
                    None => return step_note(step, Status::Failed, started, Some(format!("`{{{{{}}}}}` isn't set: capture it in an earlier step", fe.list))),
                    Some(Value::Array(a)) => a,
                    Some(Value::Null) => vec![],
                    Some(v) => vec![v],
                };
                if items.is_empty() {
                    let why = format!("skipped: `{{{{{}}}}}` is an empty list, so there was nothing to do", fe.list);
                    return StepResult { note: Some(why), ..step_note(step, Status::Skipped, started, None) };
                }
                if items.len() > fe.max {
                    let msg = format!(
                        "`{{{{{}}}}}` has {} items, more than the {} this step runs at most, so none were run. If that many is expected, write `(at most {})` after the list",
                        fe.list,
                        items.len(),
                        fe.max,
                        items.len()
                    );
                    return step_note(step, Status::Failed, started, Some(msg));
                }
                for (i, v) in items.into_iter().enumerate() {
                    let label = item_label(&v);
                    cases.push(Case { var: "item", value: v, row: i + 1, table: 0, table_name: None, label, line: step.line, prefix: format!("{prefix}item{}-", i + 1) });
                }
            } else if step.tables.is_empty() {
                if let Some(skip) = only_if(step, &st.vars) {
                    return match skip {
                        Ok(why) => StepResult { note: Some(why), ..step_note(step, Status::Skipped, started, None) },
                        Err(e) => step_note(step, Status::Failed, started, Some(e)),
                    };
                }
                return self.exec_step_once(st, step, prefix).await;
            } else {
                let several = step.drivers().len() > 1;
                for (t, i) in step.row_refs() {
                    let Some(value) = step.row_value_in(t, i) else { continue };
                    cases.push(Case {
                        var: "row",
                        value,
                        row: i + 1,
                        table: t,
                        table_name: several.then(|| step.table_name(t)),
                        label: step.row_label_in(t, i),
                        line: step.tables[t].rows[i].line,
                        prefix: if t == 0 { format!("{prefix}row{}-", i + 1) } else { format!("{prefix}t{}row{}-", t + 1, i + 1) },
                    });
                }
            }
            // Each case starts from the values captured before the step; nothing carries between them.
            let before = st.vars.captured.clone();
            let mut rows = Vec::new();
            for c in cases {
                st.vars.captured = before.clone();
                st.vars.set(c.var, c.value);
                let result = match only_if(step, &st.vars) {
                    None => self.exec_step_once(st, step, &c.prefix).await,
                    Some(Ok(why)) => StepResult { note: Some(why), ..step_note(step, Status::Skipped, Instant::now(), None) },
                    Some(Err(e)) => step_note(step, Status::Failed, Instant::now(), Some(e)),
                };
                rows.push(RowResult { row: c.row, table: c.table, table_name: c.table_name, label: c.label, line: c.line, result });
            }
            st.vars.captured = before;
            let failed: Vec<&RowResult> = rows.iter().filter(|r| r.result.status == Status::Failed).collect();
            let status = if !failed.is_empty() {
                Status::Failed
            } else if rows.iter().all(|r| r.result.status == Status::Skipped) {
                Status::Skipped
            } else {
                Status::Passed
            };
            let error = failed.first().map(|f| {
                let e = f.result.error.clone().unwrap_or(StepError { message: "failed".into(), expected: None, actual: None });
                let what = if step.for_each.is_some() { "item" } else { "row" };
                let more = if failed.len() > 1 { format!(" ({} of {} {what}s failed)", failed.len(), rows.len()) } else { String::new() };
                let of = f.table_name.as_ref().map(|n| format!(" of {n}")).unwrap_or_default();
                StepError { message: format!("{what} {}{of} ({}): {}{more}", f.row, f.label, e.message), expected: e.expected, actual: e.actual }
            });
            StepResult {
                key: step.key.clone(),
                status,
                duration_ms: started.elapsed().as_millis() as u64,
                error,
                note: None,
                records: vec![],
                captured: vec![],
                screenshot: None,
                snapshot: None,
                page_url: None,
                console: vec![],
                children: vec![],
                rows,
            }
        })
    }

    fn exec_step_once<'a>(&'a self, st: &'a mut FlowState, step: &'a Step, prefix: &'a str) -> StepFuture<'a> {
        Box::pin(async move {
            let started = Instant::now();
            // A `requests:` check covers what the page did during this step.
            for s in st.sessions.values() {
                s.clear_requests().await;
            }
            let mut r = StepResult {
                key: step.key.clone(),
                status: Status::Passed,
                duration_ms: 0,
                error: None,
                note: None,
                records: vec![],
                captured: vec![],
                screenshot: None,
                snapshot: None,
                page_url: None,
                console: vec![],
                children: vec![],
                rows: vec![],
            };
            let mut used_browser: Option<String> = None;
            let mut wants_shot = false;
            let mut ran = 0usize;
            let mut no_backend = 0usize;
            for raw in &step.blocks {
                if self.opts.prototype.is_some() && matches!(raw.kind, BlockKind::Http | BlockKind::Exec | BlockKind::Email) {
                    no_backend += 1;
                    continue;
                }
                ran += 1;
                let mut rendered = match render(&raw.value, &st.vars) {
                    Ok(v) => v,
                    Err(e) => {
                        r.error = Some(StepError { message: e, expected: None, actual: None });
                        break;
                    }
                };
                let secret_names = enocone_spec::blocks::take_secret_captures(&mut rendered);
                let block = match Block::from_raw(raw.kind, &rendered) {
                    Ok(b) => b,
                    Err(e) => {
                        r.error = Some(StepError { message: format!("{} block: {e}", raw.kind.as_str()), expected: None, actual: None });
                        break;
                    }
                };
                match block {
                    Block::Http(spec) if spec.browser_session => {
                        let auth = match self.browser_cookies(st, spec.auth.as_deref(), &spec.url).await {
                            Ok(cookie) => cookie.map(|c| vec![("Cookie".to_string(), c)]).unwrap_or_default(),
                            Err(e) => {
                                r.error = Some(StepError { message: e, expected: None, actual: None });
                                break;
                            }
                        };
                        let out = run_http(&self.http, &spec, &auth, &self.masker, self.opts.timeout).await;
                        r.records.push(Record::Http(out.record));
                        self.keep(st, &mut r, out.captured, &secret_names);
                        if let Some(e) = out.error {
                            r.error = Some(e);
                            break;
                        }
                    }
                    Block::Http(spec) => {
                        let auth = match &spec.auth {
                            Some(name) => match self.auth(name, Need::Token, &st.signing_in).await {
                                Ok(a) => a.headers,
                                Err(e) => {
                                    r.error = Some(StepError { message: format!("auth profile `{name}`: {e}"), expected: None, actual: None });
                                    break;
                                }
                            },
                            None => vec![],
                        };
                        let out = run_http(&self.http, &spec, &auth, &self.masker, self.opts.timeout).await;
                        r.records.push(Record::Http(out.record));
                        self.keep(st, &mut r, out.captured, &secret_names);
                        if let Some(e) = out.error {
                            r.error = Some(e);
                            break;
                        }
                    }
                    Block::Browser(spec) => {
                        let key = spec.as_profile.clone().unwrap_or_default();
                        let session = match self.ensure_session(st, &key).await {
                            Ok(session) => session,
                            Err(e) => {
                                r.error = Some(StepError { message: self.masker.mask(&e.to_string()), expected: None, actual: None });
                                break;
                            }
                        };
                        let t = Timeouts { action: self.opts.timeout, navigation: self.opts.navigation_timeout };
                        let out = run_browser(session, &spec, self.base_url().as_deref(), &t, &self.masker).await;
                        used_browser = Some(key);
                        wants_shot |= out.wants_screenshot;
                        r.records.extend(out.records.into_iter().map(Record::Action));
                        self.keep(st, &mut r, out.captured, &secret_names);
                        if let Some(e) = out.error {
                            r.error = Some(e);
                            break;
                        }
                    }
                    Block::Exec(spec) => {
                        if !self.project.config.run.allow_exec {
                            r.error =
                                Some(StepError { message: "exec blocks are turned off for this project; set `allow_exec = true` under [run] in enocone.toml".into(), expected: None, actual: None });
                            break;
                        }
                        let out = run_exec(&spec, &self.project.root, &self.masker, self.opts.timeout).await;
                        r.records.push(Record::Exec(out.record));
                        self.keep(st, &mut r, out.captured, &secret_names);
                        if let Some(e) = out.error {
                            r.error = Some(e);
                            break;
                        }
                    }
                    Block::Email(spec) => {
                        let mailbox = match self.mailbox() {
                            Ok(m) => m,
                            Err(e) => {
                                r.error = Some(StepError { message: e, expected: None, actual: None });
                                break;
                            }
                        };
                        let out = run_email(&self.http, &mailbox, &spec, st.started, &self.masker).await;
                        r.records.push(Record::Email(out.record));
                        self.keep(st, &mut r, out.captured, &secret_names);
                        if let Some(e) = out.error {
                            r.error = Some(e);
                            break;
                        }
                    }
                    Block::Use(u) => {
                        let fragment = match self.project.load_fragment(&u.fragment) {
                            Ok(f) => f,
                            Err(e) => {
                                r.error = Some(StepError { message: e.to_string(), expected: None, actual: None });
                                break;
                            }
                        };
                        for (k, v) in u.params {
                            st.vars.set(&k, v);
                        }
                        let child_prefix = format!("{prefix}{}-", step.key);
                        let mut failed = false;
                        for child in fragment.steps() {
                            if failed {
                                r.children.push(ChildStep { intent: child.intent.clone(), result: note_only(child, Status::Skipped, "not run because an earlier step failed") });
                                continue;
                            }
                            let res = self.exec_step(st, child, &child_prefix).await;
                            if res.status == Status::Failed {
                                failed = true;
                                r.error =
                                    Some(StepError { message: format!("{} › {}", fragment.title, res.error.as_ref().map(|e| e.message.clone()).unwrap_or_default()), expected: None, actual: None });
                            }
                            r.children.push(ChildStep { intent: child.intent.clone(), result: res });
                        }
                        if failed {
                            break;
                        }
                    }
                }
            }
            if r.error.is_some() {
                r.status = Status::Failed;
            } else if no_backend > 0 {
                r.note = Some("prototype: no backend".into());
                if ran == 0 {
                    r.status = Status::Skipped;
                }
            }
            // Evidence from the browser the step used.
            if let Some(key) = used_browser
                && let Some(s) = st.sessions.get(&key)
            {
                r.page_url = s.url().await.ok().map(|u| self.masker.mask(&u));
                let failed = r.status == Status::Failed;
                let shoot = wants_shot
                    || match self.opts.screenshots {
                        Screenshots::EveryStep => true,
                        Screenshots::OnFailure => failed,
                        Screenshots::Never => false,
                    };
                if shoot && let Ok(png) = s.screenshot().await {
                    let rel = format!("assets/{}/{}-{}{}.png", sanitize(&st.flow_id), st.attempt, prefix, step.key);
                    let path = self.opts.out_dir.join(&rel);
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    if std::fs::write(&path, png).is_ok() {
                        r.screenshot = Some(rel);
                    }
                }
                if failed {
                    r.snapshot = s.snapshot().await.ok().map(|t| self.masker.mask(&t));
                }
                r.console = s.take_console().await.into_iter().map(|c| self.masker.mask(&c)).collect();
            }
            r.duration_ms = started.elapsed().as_millis() as u64;
            r
        })
    }

    /// The flow's browser for `profile`, opened (and signed in) on first use.
    async fn ensure_session<'s>(&self, st: &'s mut FlowState, profile: &str) -> anyhow::Result<&'s dyn Session> {
        let session = match st.sessions.entry(profile.to_string()) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => e.insert(self.new_flow_session(profile, &st.signing_in).await?),
        };
        Ok(&**session)
    }

    /// A new browser for a flow, with `profile`'s kept sign-in when one is named.
    async fn new_flow_session(&self, profile: &str, signing_in: &[String]) -> anyhow::Result<Box<dyn Session>> {
        let driver = self.driver.as_ref().ok_or_else(|| anyhow!("this step needs a browser, but no browser driver is running"))?;
        let storage = if profile.is_empty() {
            None
        } else {
            let a = self.auth(profile, Need::SignIn, signing_in).await.map_err(|e| anyhow!("auth profile `{profile}`: {e}"))?;
            if a.storage.is_none() {
                bail!("auth profile `{profile}` is not a browser login, so it can't sign in a browser (use type = \"browser\")");
            }
            a.storage
        };
        open_browser(driver, SessionOptions { storage_state: storage, routes: Arc::clone(&self.opts.routes) }, self.opts.timeout).await?.into_session()
    }

    /// The `Cookie` header a signed-in browser would send to `url`: from the
    /// flow's browser for that profile if it is open, else the profile's kept
    /// sign-in; without a profile, from the flow's own browser. Cookie values
    /// are masked from then on.
    async fn browser_cookies(&self, st: &FlowState, profile: Option<&str>, url: &str) -> Result<Option<String>, String> {
        let key = profile.unwrap_or("");
        let state = match st.sessions.get(key) {
            Some(session) => session.storage_state().await.map_err(|e| format!("reading the browser's cookies: {e}"))?,
            None => match profile {
                Some(name) => match self.auth(name, Need::SignIn, &st.signing_in).await {
                    Ok(AuthState { storage: Some(s), .. }) => s,
                    Ok(_) => return Err(format!("`session: browser` needs a browser sign-in, but auth profile `{name}` isn't one (use type = \"browser\")")),
                    Err(e) => return Err(format!("auth profile `{name}`: {e}")),
                },
                None => {
                    return Err(
                        "`session: browser` sends the cookies of a signed-in browser, but there is none yet: add a browser step before this one, or name a browser sign-in profile with `auth:`".into(),
                    );
                }
            },
        };
        let url = reqwest::Url::parse(url).map_err(|e| format!("`{url}` isn't a URL: {e}"))?;
        let cookies = cookies_for(&state, &url, chrono::Utc::now().timestamp() as f64);
        for (_, v) in &cookies {
            self.masker.add(v);
        }
        Ok((!cookies.is_empty()).then(|| cookies.iter().map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join("; ")))
    }

    /// Resolve an auth profile once per run, as `crate::auth` describes:
    /// one attempt in flight per profile, whose one outcome every use waiting
    /// on it gets; other profiles never wait for it. A use that needs only
    /// the sign-in (`Need::SignIn`) gets the kept sign-in at once once there
    /// is one, and waits only for the first sign-in.
    ///
    /// `signing_in` names the profiles whose logins this use runs inside. A
    /// login that signs in as one of them again fails at once. So does one
    /// that would wait on an attempt that is itself waiting, directly or
    /// through other logins, on this login: each such wait is recorded while
    /// it lasts, and the error names the chain. A sign-in run inside another
    /// profile's login also counts against that login's budget.
    async fn auth(&self, name: &str, need: Need, signing_in: &[String]) -> Result<AuthState, String> {
        if let Some(at) = signing_in.iter().position(|p| p == name) {
            return Err(login_loop(&[&signing_in[at..], &[name.to_string()]].concat()));
        }
        let _waiting = signing_in.last().map(|from| self.start_waiting(from, name)).transpose().map_err(|path| login_loop(&path))?;
        let profile = self.profile_auth(name);
        loop {
            match profile.next(need, |kept| self.token_is_stale(name, kept)) {
                Next::Answer(outcome) => return outcome,
                Next::Wait(attempt) => {
                    if let Some(outcome) = outcome_of(attempt).await {
                        return outcome;
                    }
                }
                Next::SignIn(lead) => {
                    if matches!(self.project.config.auth.get(name), Some(AuthProfile::Browser { .. } | AuthProfile::Oauth2 { .. })) {
                        (self.events)(Event::Log(format!("signing in as `{name}`")));
                    }
                    let budget = self.opts.timeout * SIGN_IN_STEPS;
                    let deadline = tokio::time::Instant::now() + budget;
                    let signed_in = tokio::time::timeout_at(deadline, self.resolve_auth(name, signing_in, deadline))
                        .await
                        .unwrap_or_else(|_| Err(anyhow!("signing in gave no answer within {budget:?}")))
                        .map_err(|e| self.masker.mask(&format!("{e:#}")));
                    return lead.finish(signed_in);
                }
                Next::Refresh(kept, lead) => {
                    (self.events)(Event::Log(format!("refreshing the token for `{name}`")));
                    let refreshed = self.refresh_token(name, kept).await.map_err(|e| self.masker.mask(&format!("{e:#}")));
                    return lead.finish(refreshed);
                }
            }
        }
    }

    /// Record that `from`'s login waits on `to`'s attempt, unless `to` is
    /// already waiting, directly or through other logins, on `from`: then the
    /// chain from `from` round to itself again.
    fn start_waiting(&self, from: &str, to: &str) -> Result<Waiting<'_>, Vec<String>> {
        let mut waits = self.waits.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut path = vec![from.to_string(), to.to_string()];
        let mut at = to;
        // Recorded waits never form a loop, so this walk ends within their number.
        for _ in 0..waits.len() {
            let Some(next) = waits.get(at) else { break };
            path.push(next.clone());
            if next == from {
                return Err(path);
            }
            at = next;
        }
        waits.insert(from.to_string(), to.to_string());
        Ok(Waiting { waits: &self.waits, from: from.to_string() })
    }

    fn profile_auth(&self, name: &str) -> Arc<ProfileAuth> {
        let mut slots = self.auth.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(slots.entry(name.to_string()).or_default())
    }

    /// Whether a browser profile's token is older than its `refresh` age.
    fn token_is_stale(&self, name: &str, state: &AuthState) -> bool {
        match self.project.config.auth.get(name) {
            Some(AuthProfile::Browser { token: Some(t), .. }) => t.refresh.as_deref().map(parse_duration_str).is_some_and(|age| age.is_ok_and(|age| state.fetched.elapsed() >= age)),
            _ => false,
        }
    }

    fn render_str(&self, s: &str) -> anyhow::Result<String> {
        let vars = self.vars(0);
        match enocone_spec::template::render_str(s, &vars).map_err(|e| anyhow!(e))? {
            Value::String(s) => Ok(s),
            other => Ok(value_preview(&other)),
        }
    }

    /// Read a browser profile's token again from its saved session (no new
    /// login). The whole refresh gives up after the step timeout. Its browser
    /// is closed in the background on every path, so a hung close holds up
    /// no use, and a browser that opens only after the refresh gave up is
    /// closed when it opens.
    async fn refresh_token(&self, name: &str, state: AuthState) -> anyhow::Result<AuthState> {
        let Some(AuthProfile::Browser { token: Some(t), .. }) = self.project.config.auth.get(name).cloned() else { bail!("not a browser profile with a token") };
        let url = self.render_str(&t.url)?;
        let fail = |e: anyhow::Error| {
            anyhow!(
                "refreshing the token for `{name}` from {url} failed: {e:#}. The saved browser session may have expired too; sign-in runs once per run, so lengthen the session or shorten the suite"
            )
        };
        let driver = self.driver.as_ref().ok_or_else(|| anyhow!("refreshing needs a browser, but no browser driver is running"))?;
        let timeout = self.opts.timeout;
        let deadline = tokio::time::Instant::now() + timeout;
        let refreshed = tokio::time::timeout_at(deadline, async {
            let browser = open_browser(driver, SessionOptions { storage_state: state.storage.clone(), routes: Arc::clone(&self.opts.routes) }, timeout).await?;
            let session = browser.session()?;
            // The fetch needs only a committed document on the token's own URL, not a loaded page.
            session.goto_committed(&url, self.opts.navigation_timeout).await?;
            let tok = self.read_token(session, &url, &t.path, deadline).await?;
            let storage = session.storage_state().await.ok().or(state.storage.clone());
            anyhow::Ok(AuthState { headers: vec![("Authorization".to_string(), format!("Bearer {tok}"))], storage, fetched: tokio::time::Instant::now() })
        })
        .await;
        refreshed.unwrap_or_else(|_| Err(anyhow!("the browser gave no answer within {timeout:?}"))).map_err(fail)
    }

    /// Fetch the token from inside a signed-in page, giving up at `deadline`.
    async fn read_token(&self, session: &dyn Session, url: &str, path: &str, deadline: tokio::time::Instant) -> anyhow::Result<String> {
        let allowed = deadline.saturating_duration_since(tokio::time::Instant::now());
        let js = format!("fetch({}, {{ credentials: 'include', signal: AbortSignal.timeout({}) }}).then(r => r.json())", serde_json::to_string(url)?, allowed.as_millis());
        let body = tokio::time::timeout_at(deadline, session.eval(&js))
            .await
            .map_err(|_| anyhow!("fetching token from {url} gave no answer within {allowed:?}"))?
            .with_context(|| format!("fetching token from {url}"))?;
        let path = if path.starts_with('$') { path.to_string() } else { format!("$.{path}") };
        let tok = json_at(&body, &path).and_then(|v| v.as_str().map(String::from)).ok_or_else(|| anyhow!("no token at {path} in {url} (are you signed in?)"))?;
        self.masker.add(&tok);
        Ok(tok)
    }

    /// Sign in as `name`; a browser login gives up its last reads at `deadline`.
    async fn resolve_auth(&self, name: &str, signing_in: &[String], deadline: tokio::time::Instant) -> anyhow::Result<AuthState> {
        let profile =
            self.project.config.auth.get(name).ok_or_else(|| anyhow!("not defined in enocone.toml (defined: {})", self.project.config.auth.keys().cloned().collect::<Vec<_>>().join(", ")))?.clone();
        match profile {
            AuthProfile::Header { header, value } => {
                let v = self.render_str(&value)?;
                self.masker.add(&v);
                Ok(AuthState { headers: vec![(header, v)], storage: None, fetched: tokio::time::Instant::now() })
            }
            AuthProfile::Bearer { token } => {
                let t = self.render_str(&token)?;
                self.masker.add(&t);
                Ok(AuthState { headers: vec![("Authorization".into(), format!("Bearer {t}"))], storage: None, fetched: tokio::time::Instant::now() })
            }
            AuthProfile::Oauth2 { token_url, grant, client_id, client_secret, username, password, scope } => {
                let mut form = vec![("grant_type".to_string(), grant.clone()), ("client_id".into(), self.render_str(&client_id)?)];
                for (k, v) in [("client_secret", client_secret), ("username", username), ("password", password), ("scope", scope)] {
                    if let Some(v) = v {
                        let v = self.render_str(&v)?;
                        if k != "username" && k != "scope" {
                            self.masker.add(&v);
                        }
                        form.push((k.into(), v));
                    }
                }
                let url = self.render_str(&token_url)?;
                let resp = self.http.post(&url).form(&form).send().await.with_context(|| format!("token request to {url}"))?;
                let status = resp.status();
                let body: Value = resp.json().await.unwrap_or(Value::Null);
                let token = body.get("access_token").and_then(Value::as_str).ok_or_else(|| anyhow!("token endpoint returned {status} without access_token: {body}"))?;
                self.masker.add(token);
                Ok(AuthState { headers: vec![("Authorization".into(), format!("Bearer {token}"))], storage: None, fetched: tokio::time::Instant::now() })
            }
            AuthProfile::Browser { login, params, token } => {
                let driver = self.driver.as_ref().ok_or_else(|| anyhow!("needs a browser to sign in, but no browser driver is running"))?;
                let fragment = self.project.load_fragment(&login)?;
                if let Some(path) = self.login_loop_from(name, &fragment) {
                    bail!(login_loop(&path));
                }
                let params = params.iter().map(|(k, v)| Ok((k.clone(), self.render_str(v)?))).collect::<anyhow::Result<Vec<_>>>()?;
                let browser = open_browser(driver, SessionOptions { storage_state: None, routes: Arc::clone(&self.opts.routes) }, self.opts.timeout).await?;
                let mut st = FlowState {
                    vars: self.vars(0),
                    sessions: HashMap::new(),
                    flow_id: format!("auth-{name}"),
                    attempt: 1,
                    started: chrono::Utc::now(),
                    signing_in: [signing_in, &[name.to_string()]].concat(),
                    close_timeout: self.opts.timeout,
                };
                for (k, v) in params {
                    st.vars.set(&k, Value::String(v));
                }
                st.sessions.insert(String::new(), browser.into_session()?);
                // Its browsers close in the background as `st` goes.
                self.sign_in(&mut st, &fragment, token.as_ref(), deadline).await
            }
        }
    }

    /// Run a login fragment in `st`'s default browser, then read what it
    /// signed in, each read within a step timeout and by `deadline`.
    async fn sign_in(&self, st: &mut FlowState, fragment: &Flow, token: Option<&BrowserToken>, deadline: tokio::time::Instant) -> anyhow::Result<AuthState> {
        for step in fragment.steps() {
            let r = self.exec_step(st, step, "").await;
            if r.status == Status::Failed {
                bail!("login step \"{}\" failed: {}{}", step.intent, r.error.map(|e| e.message).unwrap_or_default(), r.screenshot.map(|s| format!(" (screenshot: {s})")).unwrap_or_default());
            }
        }
        let session = st.sessions.get("").ok_or_else(|| anyhow!("the login fragment's browser session is gone"))?;
        let read_by = |now: tokio::time::Instant| deadline.min(now + self.opts.timeout);
        let started = tokio::time::Instant::now();
        let storage = tokio::time::timeout_at(read_by(started), session.storage_state())
            .await
            .map_err(|_| anyhow!("reading the signed-in browser's state gave no answer within {:?}", read_by(started) - started))??;
        let headers = match token {
            Some(t) => {
                let url = self.render_str(&t.url)?;
                let tok = self.read_token(session.as_ref(), &url, &t.path, read_by(tokio::time::Instant::now())).await?;
                vec![("Authorization".to_string(), format!("Bearer {tok}"))]
            }
            None => vec![],
        };
        Ok(AuthState { headers, storage: Some(storage), fetched: tokio::time::Instant::now() })
    }

    /// The profiles a login fragment signs in as by name (`as:` in browser
    /// blocks, `auth:` in http blocks), including in the fragments it uses.
    /// Names filled in from templates are left to the check made at each use.
    fn signs_in_as(&self, fragment: &Flow, used: &mut HashSet<String>, found: &mut BTreeSet<String>) {
        for raw in fragment.steps().flat_map(|s| &s.blocks) {
            // Read as the block itself reads the name (text, a number or true/false).
            let named = |key: &str| match raw.value.get(key) {
                Some(Value::String(s)) if !s.contains("{{") => Some(s.clone()),
                Some(Value::Number(n)) => Some(n.to_string()),
                Some(Value::Bool(b)) => Some(b.to_string()),
                _ => None,
            };
            match raw.kind {
                BlockKind::Browser => found.extend(named("as")),
                BlockKind::Http => found.extend(named("auth")),
                BlockKind::Use => {
                    if let Ok(u) = UseSpec::from_value(&raw.value)
                        && used.insert(u.fragment.clone())
                        && let Ok(f) = self.project.load_fragment(&u.fragment)
                    {
                        self.signs_in_as(&f, used, found);
                    }
                }
                _ => {}
            }
        }
    }

    /// How `name`'s login (`login`) reaches a sign-in as `name` again through
    /// the logins of the profiles it signs in as, if it does: `name`, the
    /// profiles in between, then `name`.
    fn login_loop_from(&self, name: &str, login: &Flow) -> Option<Vec<String>> {
        let profiles_of = |fragment: &Flow| {
            let mut found = BTreeSet::new();
            self.signs_in_as(fragment, &mut HashSet::new(), &mut found);
            found
        };
        let mut seen = HashSet::from([name.to_string()]);
        let mut paths = vec![(vec![name.to_string()], profiles_of(login))];
        while let Some((path, next)) = paths.pop() {
            for profile in next {
                let path = [path.as_slice(), std::slice::from_ref(&profile)].concat();
                if profile == name {
                    return Some(path);
                }
                if seen.insert(profile.clone())
                    && let Some(AuthProfile::Browser { login, .. }) = self.project.config.auth.get(&profile)
                    && let Ok(fragment) = self.project.load_fragment(login)
                {
                    paths.push((path, profiles_of(&fragment)));
                }
            }
        }
        None
    }

    pub async fn shutdown(&self) {
        if let Some(d) = &self.driver {
            let _ = d.shutdown().await;
        }
    }
}

fn note_only(step: &Step, status: Status, why: &str) -> StepResult {
    StepResult {
        key: step.key.clone(),
        status,
        duration_ms: 0,
        error: None,
        note: Some(why.to_string()),
        records: vec![],
        captured: vec![],
        screenshot: None,
        snapshot: None,
        page_url: None,
        console: vec![],
        children: vec![],
        rows: vec![],
    }
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect()
}

/// A step result that ran nothing: an error, or a skip.
fn step_note(step: &Step, status: Status, started: Instant, error: Option<String>) -> StepResult {
    StepResult {
        key: step.key.clone(),
        status,
        duration_ms: started.elapsed().as_millis() as u64,
        error: error.map(|message| StepError { message, expected: None, actual: None }),
        note: None,
        records: vec![],
        captured: vec![],
        screenshot: None,
        snapshot: None,
        page_url: None,
        console: vec![],
        children: vec![],
        rows: vec![],
    }
}

/// How an item of a `For each:` list is named in results: itself, or its `id` or `name`.
fn item_label(v: &Value) -> String {
    let v = match v {
        Value::Object(o) => o.get("id").or_else(|| o.get("name")).unwrap_or(v),
        _ => v,
    };
    let s = match v {
        Value::String(s) => s.clone(),
        other => value_preview(other),
    };
    if s.chars().count() > 60 { format!("{}…", s.chars().take(59).collect::<String>()) } else { s }
}

/// A step's `Only if:` condition, checked now: None to run, Ok(why) to skip,
/// Err when it can't be checked.
fn only_if(step: &Step, vars: &Vars) -> Option<Result<String, String>> {
    let text = step.only_if.as_deref()?;
    let cond = match enocone_spec::cond::parse(text) {
        Ok(c) => c,
        Err(e) => return Some(Err(format!("`Only if: {text}` can't be read: {e}"))),
    };
    match cond.eval(vars) {
        Ok((true, _)) => None,
        Ok((false, seen)) => {
            let saw: Vec<String> = seen.iter().map(|(name, v)| format!("{} was {}", name.rsplit('.').next().unwrap_or(name), vars.masker.mask(&v.to_string()))).collect();
            let saw = if saw.is_empty() { String::new() } else { format!(": {}", saw.join(", ")) };
            Some(Ok(format!("skipped: only if {text}{saw}")))
        }
        Err(e) => Some(Err(format!("`Only if: {text}`: {e}"))),
    }
}

/// The cookies in a browser's saved state that it would send to `url`:
/// matching domain and path, `Secure` only over https, and not expired.
fn cookies_for(state: &Value, url: &reqwest::Url, now: f64) -> Vec<(String, String)> {
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let path = url.path();
    let secure = url.scheme() == "https";
    let mut out = Vec::new();
    for c in state["cookies"].as_array().into_iter().flatten() {
        let s = |k: &str| c[k].as_str().unwrap_or("").to_string();
        let domain = s("domain").to_ascii_lowercase();
        let host_ok = match domain.strip_prefix('.') {
            Some(d) => host == d || host.ends_with(&format!(".{d}")),
            None => host == domain,
        };
        let cpath = s("path");
        let cpath = if cpath.is_empty() { "/".to_string() } else { cpath };
        let path_ok = path == cpath || (path.starts_with(&cpath) && (cpath.ends_with('/') || path[cpath.len()..].starts_with('/')));
        let expires = c["expires"].as_f64().unwrap_or(-1.0);
        let live = expires < 0.0 || expires > now;
        if host_ok && path_ok && live && (secure || !c["secure"].as_bool().unwrap_or(false)) {
            out.push((s("name"), s("value")));
        }
    }
    out
}

#[cfg(test)]
mod cookie_tests {
    use super::*;

    #[test]
    fn picks_the_cookies_a_browser_would_send() {
        let state = serde_json::json!({ "cookies": [
            { "name": "session", "value": "abc", "domain": "app.test", "path": "/", "expires": -1, "secure": false },
            { "name": "shared", "value": "s1", "domain": ".app.test", "path": "/", "expires": -1, "secure": false },
            { "name": "admin", "value": "x", "domain": "app.test", "path": "/admin", "expires": -1, "secure": false },
            { "name": "sec", "value": "y", "domain": "app.test", "path": "/", "expires": -1, "secure": true },
            { "name": "old", "value": "z", "domain": "app.test", "path": "/", "expires": 10.0, "secure": false },
            { "name": "other", "value": "o", "domain": "other.test", "path": "/", "expires": -1, "secure": false }
        ] });
        let names = |u: &str| cookies_for(&state, &reqwest::Url::parse(u).unwrap(), 1000.0).into_iter().map(|(n, _)| n).collect::<Vec<_>>();
        assert_eq!(names("http://app.test/api/x"), ["session", "shared"]);
        assert_eq!(names("https://app.test/admin/users"), ["session", "shared", "admin", "sec"]);
        assert_eq!(names("http://app.test/administrator"), ["session", "shared"]);
        assert_eq!(names("http://api.app.test/"), ["shared"]);
    }
}

#[cfg(test)]
mod auth_tests {
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use enocone_spec::blocks::Locator;
    use tempfile::TempDir;
    use tokio::sync::{Barrier, Notify};

    use super::*;
    use crate::auth::REFRESH_RETRY_AFTER;
    use crate::driver::{UiAction, UiQuery};

    type Reply = Pin<Box<dyn Future<Output = anyhow::Result<Value>> + Send>>;

    /// Where a token refresh's browser stops answering.
    #[derive(Clone, Copy, PartialEq)]
    enum Hang {
        /// Opening the browser, until `release` is notified.
        Open,
        /// Committing the token URL, forever.
        Commit,
        /// Reading its storage state, forever.
        Storage,
        /// Closing, forever, once the close is recorded.
        Close,
        /// Reading the page's address, forever, in a sign-in's browser.
        SignInUrl,
        /// Opening a browser without saved storage (a sign-in's, or a
        /// flow's own), until `release` is notified.
        OpenWithoutStorage,
        /// Reading its storage state, forever, in a sign-in's browser.
        SignInStorage,
    }

    /// A browser whose session fetches answer as `reply` says, by call number.
    /// A session opened with saved storage is a refresh's, where `hang` applies.
    struct FakeBrowser {
        opens: AtomicUsize,
        opened: AtomicUsize,
        closed: StdMutex<Vec<usize>>,
        closing: Notify,
        release: Notify,
        evals: AtomicUsize,
        navigations: StdMutex<Vec<String>>,
        /// The routes each session was opened with.
        routed: StdMutex<Vec<Arc<[Route]>>>,
        login_fails: bool,
        hangs: &'static [Hang],
        /// When set, each sign-in's first page load waits until this many have started.
        logins_meet: Option<Barrier>,
        /// How long each navigation takes.
        latency: Duration,
        reply: Box<dyn Fn(usize) -> Reply + Send + Sync>,
    }

    impl FakeBrowser {
        fn opens(&self) -> usize {
            self.opens.load(Ordering::SeqCst)
        }

        fn opened(&self) -> usize {
            self.opened.load(Ordering::SeqCst)
        }

        fn evals(&self) -> usize {
            self.evals.load(Ordering::SeqCst)
        }

        fn closes(&self) -> usize {
            self.closed.lock().expect("closed").len()
        }

        /// Every session opened so far is closed (some close in the
        /// background, so this waits for them), and none more than once.
        async fn assert_each_closed_once(&self) {
            let all_closed = async {
                loop {
                    let closing = self.closing.notified();
                    if self.closes() >= self.opened() {
                        break;
                    }
                    closing.await;
                }
            };
            tokio::time::timeout(Duration::from_secs(60), all_closed).await.expect("every session is closed");
            let mut closed = self.closed.lock().expect("closed").clone();
            closed.sort_unstable();
            assert_eq!(closed, (0..self.opened()).collect::<Vec<_>>(), "sessions closed, by id");
        }
    }

    struct FakeDriver(Arc<FakeBrowser>);

    struct FakeSession {
        id: usize,
        refresh: bool,
        browser: Arc<FakeBrowser>,
    }

    impl FakeSession {
        async fn hangs_at(&self, at: Hang) {
            if self.refresh && self.browser.hangs.contains(&at) {
                std::future::pending::<()>().await;
            }
        }
    }

    #[async_trait]
    impl Driver for FakeDriver {
        fn name(&self) -> String {
            "fake".into()
        }
        async fn new_session(&self, options: SessionOptions) -> anyhow::Result<Box<dyn Session>> {
            self.0.opens.fetch_add(1, Ordering::SeqCst);
            self.0.routed.lock().expect("routed").push(Arc::clone(&options.routes));
            let refresh = options.storage_state.is_some();
            if (refresh && self.0.hangs.contains(&Hang::Open)) || (!refresh && self.0.hangs.contains(&Hang::OpenWithoutStorage)) {
                self.0.release.notified().await;
            }
            let id = self.0.opened.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(FakeSession { id, refresh, browser: Arc::clone(&self.0) }))
        }
        async fn shutdown(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Session for FakeSession {
        async fn goto(&self, url: &str, _: Duration) -> anyhow::Result<()> {
            self.browser.navigations.lock().expect("navigations").push(format!("load {url}"));
            tokio::time::sleep(self.browser.latency).await;
            if let Some(meet) = &self.browser.logins_meet {
                meet.wait().await;
            }
            if self.browser.login_fails {
                bail!("the sign-in page is down");
            }
            Ok(())
        }
        async fn goto_committed(&self, url: &str, _: Duration) -> anyhow::Result<()> {
            self.browser.navigations.lock().expect("navigations").push(format!("commit {url}"));
            tokio::time::sleep(self.browser.latency).await;
            self.hangs_at(Hang::Commit).await;
            Ok(())
        }
        async fn act(&self, _: &Locator, _: UiAction, _: Duration) -> anyhow::Result<()> {
            bail!("not used")
        }
        async fn press(&self, _: &str) -> anyhow::Result<()> {
            bail!("not used")
        }
        async fn query(&self, _: &Locator, _: UiQuery) -> anyhow::Result<Value> {
            bail!("not used")
        }
        async fn url(&self) -> anyhow::Result<String> {
            if !self.refresh && self.browser.hangs.contains(&Hang::SignInUrl) {
                std::future::pending::<()>().await;
            }
            Ok("http://app.test/".into())
        }
        async fn title(&self) -> anyhow::Result<String> {
            Ok(String::new())
        }
        async fn snapshot(&self) -> anyhow::Result<String> {
            Ok(String::new())
        }
        async fn screenshot(&self) -> anyhow::Result<Vec<u8>> {
            bail!("not used")
        }
        async fn eval(&self, _: &str) -> anyhow::Result<Value> {
            let n = self.browser.evals.fetch_add(1, Ordering::SeqCst);
            (self.browser.reply)(n).await
        }
        async fn storage_state(&self) -> anyhow::Result<Value> {
            self.hangs_at(Hang::Storage).await;
            if !self.refresh && self.browser.hangs.contains(&Hang::SignInStorage) {
                std::future::pending::<()>().await;
            }
            Ok(serde_json::json!({
                "cookies": [{ "name": "sid", "value": "s1", "domain": "app.test", "path": "/", "expires": -1, "secure": false }],
                "origins": []
            }))
        }
        async fn take_console(&self) -> Vec<String> {
            vec![]
        }
        async fn close(&self) -> anyhow::Result<()> {
            self.browser.closed.lock().expect("closed").push(self.id);
            self.browser.closing.notify_waiters();
            self.hangs_at(Hang::Close).await;
            Ok(())
        }
    }

    fn token(n: usize) -> Reply {
        let body = serde_json::json!({ "accessToken": format!("t{n}") });
        Box::pin(async move { Ok(body) })
    }

    fn stalled(_: usize) -> Reply {
        Box::pin(async { Err(anyhow!("the page stalled")) })
    }

    /// The engine under test, with the project folder it reads its login from.
    struct Fixture {
        engine: Arc<Engine>,
        browser: Arc<FakeBrowser>,
        _root: TempDir,
    }

    const OPEN_SIGN_IN: &str = "1. **Open the sign-in page**\n   ```browser\n   - goto: http://app.test/login\n   ```\n";

    #[derive(Default)]
    struct Setup {
        refresh: &'static str,
        params: &'static str,
        login_fails: bool,
        hangs: &'static [Hang],
        logins_meet: usize,
        latency: Duration,
        /// The steps of `web`'s login, instead of opening the sign-in page.
        login: Option<&'static str>,
        /// More of enocone.toml, and the fragments it names.
        config: &'static str,
        fragments: &'static [(&'static str, &'static str)],
    }

    /// An engine whose `web` profile signs in through the fake browser and
    /// refreshes its token when it is `refresh` old; `key` is a fixed header.
    fn fixture(setup: Setup, reply: impl Fn(usize) -> Reply + Send + Sync + 'static) -> Fixture {
        let root = TempDir::new().expect("project folder");
        std::fs::create_dir_all(root.path().join("fragments")).expect("fragments folder");
        let Setup { refresh, params, login_fails, hangs, logins_meet, latency, login, config, fragments } = setup;
        for (name, steps) in [("sign-in", login.unwrap_or(OPEN_SIGN_IN))].into_iter().chain(fragments.iter().copied()) {
            std::fs::write(root.path().join(format!("fragments/{name}.flow.md")), format!("# {name}\n\n## When\n\n{steps}")).expect("fragment");
        }
        let config = toml::from_str(&format!(
            r#"
            project = "auth-test"
            [auth.web]
            type = "browser"
            login = "sign-in"
            {params}
            token = {{ url = "http://app.test/api/session", path = "$.accessToken", refresh = "{refresh}" }}
            [auth.key]
            type = "header"
            header = "X-Key"
            value = "k"
            {config}
            "#
        ))
        .expect("config");
        let project = Project::new(root.path().to_path_buf(), config).expect("project");
        let opts = RunOptions::from_project(&project, "local".into(), root.path().join("out")).expect("options");
        let browser = Arc::new(FakeBrowser {
            opens: AtomicUsize::new(0),
            opened: AtomicUsize::new(0),
            closed: StdMutex::new(vec![]),
            closing: Notify::new(),
            release: Notify::new(),
            evals: AtomicUsize::new(0),
            navigations: StdMutex::new(vec![]),
            routed: StdMutex::new(vec![]),
            login_fails,
            hangs,
            logins_meet: (logins_meet > 0).then(|| Barrier::new(logins_meet)),
            latency,
            reply: Box::new(reply),
        });
        let driver: Arc<dyn Driver> = Arc::new(FakeDriver(Arc::clone(&browser)));
        let engine = Engine::new(project, opts, Some(driver), Arc::new(|_| {})).expect("engine");
        Fixture { engine: Arc::new(engine), browser, _root: root }
    }

    fn refreshing(refresh: &'static str, reply: impl Fn(usize) -> Reply + Send + Sync + 'static) -> Fixture {
        fixture(Setup { refresh, ..Setup::default() }, reply)
    }

    impl Fixture {
        async fn token(&self, name: &str) -> Result<AuthState, String> {
            self.engine.auth(name, Need::Token, &[]).await
        }
    }

    fn bearer(r: &Result<AuthState, String>) -> Option<String> {
        r.as_ref().ok().and_then(|a| a.headers.iter().find(|(k, _)| k == "Authorization").map(|(_, v)| v.clone()))
    }

    /// Fetch `n` signals `entered`, waits for `gate`, then answers as `answer` says.
    fn gated(n: usize, entered: &Arc<Notify>, gate: &Arc<Notify>, answer: fn(usize) -> Reply) -> impl Fn(usize) -> Reply + Send + Sync + 'static {
        let (entered, gate) = (Arc::clone(entered), Arc::clone(gate));
        move |i| {
            let (entered, gate) = (Arc::clone(&entered), Arc::clone(&gate));
            Box::pin(async move {
                if i != n {
                    return token(i).await;
                }
                entered.notify_one();
                gate.notified().await;
                answer(i).await
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_refresh_answers_until_its_cooldown_then_is_retried() {
        let f = refreshing("0s", |n| if n == 1 { stalled(n) } else { token(n) });
        assert_eq!(bearer(&f.token("web").await).as_deref(), Some("Bearer t0"));
        let failed = f.token("web").await.err().expect("the refresh fails");
        assert!(failed.contains("refreshing the token for `web`") && failed.contains("the page stalled"), "{failed}");
        tokio::time::advance(REFRESH_RETRY_AFTER - Duration::from_millis(1)).await;
        assert_eq!(f.token("web").await.err().as_deref(), Some(failed.as_str()), "the failure answers within its cooldown");
        assert_eq!((f.browser.opened(), f.browser.evals()), (2, 2), "no refresh was tried within the cooldown");
        tokio::time::advance(Duration::from_millis(1)).await;
        assert_eq!(bearer(&f.token("web").await).as_deref(), Some("Bearer t2"));
        assert_eq!(f.browser.opened(), 3);
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn uses_queued_behind_a_failing_refresh_share_its_failure() {
        let (entered, gate) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        let f = refreshing("60s", gated(1, &entered, &gate, stalled));
        f.token("web").await.expect("signs in");
        tokio::time::advance(Duration::from_secs(61)).await;
        let all = async {
            tokio::join!(f.token("web"), f.token("web"), f.token("web"), async {
                entered.notified().await;
                gate.notify_one();
            })
        };
        let (a, b, c, ()) = tokio::time::timeout(Duration::from_secs(60), all).await.expect("the queued uses are answered");
        let failed = a.err().expect("the refresh fails");
        assert!(failed.contains("the page stalled"), "{failed}");
        assert_eq!(b.err().as_deref(), Some(failed.as_str()));
        assert_eq!(c.err().as_deref(), Some(failed.as_str()));
        assert_eq!((f.browser.opened(), f.browser.evals()), (2, 2), "one sign-in and one refresh");
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn every_session_is_opened_with_the_projects_routes() {
        let f = fixture(Setup { refresh: "0s", config: "[[run.routes]]\nurl = \"**/paid/**\"\nabort = true", ..Setup::default() }, token);
        f.token("web").await.expect("signs in");
        f.token("web").await.expect("refreshes");
        drop(f.engine.open_session(Some("web")).await.expect("explores"));
        let routes: Arc<[Route]> = vec![Route { url: "**/paid/**".into(), answer: crate::driver::RouteAnswer::Abort }].into();
        let routed = f.browser.routed.lock().expect("routed").clone();
        assert!(routed.len() >= 3, "the sign-in, the refresh and the exploring session: {routed:?}");
        assert!(routed.iter().all(|r| *r == routes), "{routed:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn a_good_refresh_clears_an_earlier_failure() {
        let f = refreshing("0s", |n| if n == 1 { stalled(n) } else { token(n) });
        f.token("web").await.expect("signs in");
        f.token("web").await.err().expect("the refresh fails");
        tokio::time::advance(REFRESH_RETRY_AFTER).await;
        assert_eq!(bearer(&f.token("web").await).as_deref(), Some("Bearer t2"));
        assert_eq!(bearer(&f.token("web").await).as_deref(), Some("Bearer t3"), "the next stale use refreshes at once");
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_refresh_does_not_hold_up_other_profiles() {
        let (entered, gate) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        let f = refreshing("0s", gated(1, &entered, &gate, token));
        f.token("web").await.expect("signs in");
        let (slow, other) = tokio::join!(f.token("web"), async {
            entered.notified().await;
            let other = tokio::time::timeout(Duration::from_secs(1), f.token("key")).await;
            gate.notify_one();
            other
        });
        let other = other.expect("another profile is not held up by the refresh").expect("header profile");
        assert_eq!(other.headers, [("X-Key".to_string(), "k".to_string())]);
        assert_eq!(bearer(&slow).as_deref(), Some("Bearer t1"));
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_uses_of_a_stale_profile_share_one_refresh() {
        let (entered, gate) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        let f = refreshing("60s", gated(1, &entered, &gate, token));
        f.token("web").await.expect("signs in");
        tokio::time::advance(Duration::from_secs(61)).await;
        let both = async {
            tokio::join!(f.token("web"), f.token("web"), async {
                entered.notified().await;
                gate.notify_one();
            })
        };
        let (first, second, ()) = tokio::time::timeout(Duration::from_secs(60), both).await.expect("the stale token is refreshed");
        assert_eq!(bearer(&first).as_deref(), Some("Bearer t1"));
        assert_eq!(bearer(&second).as_deref(), Some("Bearer t1"));
        assert_eq!(f.browser.opened(), 2);
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn the_refresh_commits_the_token_url_and_gives_up_after_the_step_timeout() {
        let f = refreshing("0s", |n| if n == 0 { token(n) } else { Box::pin(std::future::pending()) });
        f.token("web").await.expect("signs in");
        let started = tokio::time::Instant::now();
        let failed = tokio::time::timeout(Duration::from_secs(600), f.token("web")).await.expect("bounded").err().expect("times out");
        assert!(failed.contains("gave no answer within 10s"), "{failed}");
        assert_eq!(started.elapsed(), Duration::from_secs(10));
        assert_eq!(f.browser.navigations.lock().expect("navigations").as_slice(), ["load http://app.test/login", "commit http://app.test/api/session"]);
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_login_step_closes_its_browser() {
        let f = fixture(Setup { refresh: "0s", login_fails: true, ..Setup::default() }, token);
        let failed = f.token("web").await.err().expect("the login fails");
        assert!(failed.contains("login step \"Open the sign-in page\" failed") && failed.contains("the sign-in page is down"), "{failed}");
        assert_eq!(f.browser.opened(), 1);
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_token_read_at_sign_in_closes_its_browser() {
        let f = refreshing("0s", stalled);
        let failed = f.token("web").await.err().expect("the token read fails");
        assert!(failed.contains("the page stalled"), "{failed}");
        assert_eq!(f.browser.opened(), 1);
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_login_param_that_does_not_render_opens_no_browser() {
        let f = fixture(Setup { refresh: "0s", params: r#"params = { user = "{{nope}}" }"#, ..Setup::default() }, token);
        let failed = f.token("web").await.err().expect("the param does not render");
        assert!(failed.contains("unknown variable `nope`"), "{failed}");
        assert_eq!(f.browser.opened(), 0);
        f.browser.assert_each_closed_once().await;
    }

    /// Signs in, then refreshes a stale token whose browser stops at `hangs`:
    /// the refresh gives up after the step timeout, the uses queued behind it
    /// share its failure, and the failure answers through its cooldown.
    async fn a_refresh_hung_at(hangs: &'static [Hang]) -> Fixture {
        let f = fixture(Setup { refresh: "0s", hangs, ..Setup::default() }, token);
        f.token("web").await.expect("signs in");
        let started = tokio::time::Instant::now();
        let (first, queued) = tokio::time::timeout(Duration::from_secs(600), async { tokio::join!(f.token("web"), f.token("web")) }).await.expect("the refresh is bounded");
        let failed = first.err().expect("the refresh gives up");
        assert!(failed.contains("refreshing the token for `web`") && failed.contains("the browser gave no answer within 10s"), "{failed}");
        assert_eq!(started.elapsed(), Duration::from_secs(10), "one refresh, bounded by the step timeout");
        assert_eq!(queued.err().as_deref(), Some(failed.as_str()), "the queued use shares the failure");
        tokio::time::advance(REFRESH_RETRY_AFTER - Duration::from_millis(1)).await;
        assert_eq!(f.token("web").await.err().as_deref(), Some(failed.as_str()), "the failure answers within its cooldown");
        assert_eq!(f.browser.opens(), 2, "one sign-in and one refresh");
        f
    }

    #[tokio::test(start_paused = true)]
    async fn a_refresh_whose_browser_does_not_open_gives_up_and_closes_it_when_it_opens() {
        let f = a_refresh_hung_at(&[Hang::Open]).await;
        assert_eq!(f.browser.opened(), 1, "only the sign-in's browser has opened");
        f.browser.assert_each_closed_once().await;
        let closed = f.browser.closing.notified();
        f.browser.release.notify_one();
        tokio::time::timeout(Duration::from_secs(60), closed).await.expect("the late browser is closed");
        assert_eq!(f.browser.opened(), 2);
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_refresh_whose_token_url_does_not_commit_gives_up_and_closes_its_browser() {
        let f = a_refresh_hung_at(&[Hang::Commit]).await;
        assert_eq!(f.browser.evals(), 1, "no token was read");
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_refresh_whose_storage_state_does_not_answer_gives_up_and_closes_its_browser() {
        let f = a_refresh_hung_at(&[Hang::Storage]).await;
        assert_eq!(f.browser.evals(), 2, "the token was read before the storage state hung");
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_refresh_whose_close_hangs_answers_its_queued_uses_at_the_step_timeout() {
        let f = a_refresh_hung_at(&[Hang::Storage, Hang::Close]).await;
        f.browser.assert_each_closed_once().await;
    }

    fn flow_state(engine: &Engine) -> FlowState {
        FlowState { vars: engine.vars(0), sessions: HashMap::new(), flow_id: "flow".into(), attempt: 1, started: chrono::Utc::now(), signing_in: vec![], close_timeout: engine.opts.timeout }
    }

    #[tokio::test(start_paused = true)]
    async fn browser_uses_do_not_wait_for_a_refresh_in_flight() {
        let (entered, gate) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        let f = refreshing("0s", gated(1, &entered, &gate, token));
        f.token("web").await.expect("signs in");
        let (refreshed, ()) = tokio::join!(f.token("web"), async {
            entered.notified().await;
            let browser_uses = async {
                f.engine.new_flow_session("web", &[]).await.expect("a browser signed in as `web`").close().await.expect("closes");
                let cookies = f.engine.browser_cookies(&flow_state(&f.engine), Some("web"), "http://app.test/api/x").await.expect("the kept sign-in's cookies");
                assert_eq!(cookies.as_deref(), Some("sid=s1"));
                f.engine.open_session(Some("web")).await.expect("an exploring browser signed in as `web`").close().await.expect("closes");
            };
            tokio::time::timeout(Duration::from_secs(1), browser_uses).await.expect("browser uses don't wait for the refresh");
            gate.notify_one();
        });
        assert_eq!(bearer(&refreshed).as_deref(), Some("Bearer t1"));
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn browser_uses_keep_the_sign_in_through_a_refresh_cooldown() {
        let f = refreshing("0s", |n| if n == 1 { stalled(n) } else { token(n) });
        f.token("web").await.expect("signs in");
        let failed = f.token("web").await.err().expect("the refresh fails");
        let session = f.engine.new_flow_session("web", &[]).await.expect("a browser signed in as `web`");
        session.close().await.expect("closes");
        let cookies = f.engine.browser_cookies(&flow_state(&f.engine), Some("web"), "http://app.test/api/x").await.expect("the kept sign-in's cookies");
        assert_eq!(cookies.as_deref(), Some("sid=s1"));
        f.engine.open_session(Some("web")).await.expect("an exploring browser signed in as `web`").close().await.expect("closes");
        assert_eq!(f.browser.evals(), 2, "no use refreshed the token");
        assert_eq!(f.token("web").await.err().as_deref(), Some(failed.as_str()), "uses that need the token still fail within the cooldown");
        f.browser.assert_each_closed_once().await;
    }

    const AS_WEB: &str = "1. **Open the app as web**\n   ```browser\n   as: web\n   steps:\n     - goto: http://app.test/\n   ```\n";

    #[tokio::test(start_paused = true)]
    async fn a_login_that_signs_in_as_its_own_profile_fails_without_opening_a_browser() {
        let f = fixture(Setup { refresh: "0s", login: Some(AS_WEB), ..Setup::default() }, token);
        let failed = tokio::time::timeout(Duration::from_secs(60), f.token("web")).await.expect("no wait").err().expect("the login is refused");
        assert!(failed.contains("its login signs in as itself (web → web)"), "{failed}");
        assert_eq!(f.browser.opens(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_login_whose_used_fragment_signs_in_as_its_profile_fails() {
        let f = fixture(Setup { refresh: "0s", login: Some("1. **Sign in**\n   ```use\n   as-web\n   ```\n"), fragments: &[("as-web", AS_WEB)], ..Setup::default() }, token);
        let failed = f.token("web").await.err().expect("the login is refused");
        assert!(failed.contains("(web → web)"), "{failed}");
        assert_eq!(f.browser.opens(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn logins_that_sign_in_as_each_other_fail_at_once_even_when_started_together() {
        let f = fixture(
            Setup {
                refresh: "0s",
                login: Some("1. **Open the admin app**\n   ```browser\n   as: admin\n   steps:\n     - goto: http://app.test/admin\n   ```\n"),
                config: "[auth.admin]\ntype = \"browser\"\nlogin = \"admin-sign-in\"",
                fragments: &[("admin-sign-in", "1. **Read who web is**\n   ```http\n   GET: http://app.test/api/me\n   auth: web\n   ```\n")],
                ..Setup::default()
            },
            token,
        );
        let (web, admin) = tokio::time::timeout(Duration::from_secs(60), async { tokio::join!(f.token("web"), f.token("admin")) }).await.expect("neither waits for the other");
        let (web, admin) = (web.err().expect("web is refused"), admin.err().expect("admin is refused"));
        assert!(web.contains("(web → admin → web)"), "{web}");
        assert!(admin.contains("(admin → web → admin)"), "{admin}");
        assert_eq!(f.browser.opens(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_login_that_signs_in_as_its_own_profile_through_a_template_fails() {
        let f = fixture(
            Setup {
                refresh: "0s",
                params: r#"params = { who = "web" }"#,
                login: Some("1. **Open the app as whoever**\n   ```browser\n   as: \"{{who}}\"\n   steps:\n     - goto: http://app.test/\n   ```\n"),
                ..Setup::default()
            },
            token,
        );
        let failed = tokio::time::timeout(Duration::from_secs(60), f.token("web")).await.expect("no wait").err().expect("the login is refused");
        assert!(failed.contains("login step \"Open the app as whoever\" failed") && failed.contains("its login signs in as itself (web → web)"), "{failed}");
        assert_eq!(f.browser.opened(), 1);
        f.browser.assert_each_closed_once().await;
    }

    const OPEN_SIGN_IN_THEN_AS_WHO: &str = "1. **Open the sign-in page**\n   ```browser\n   - goto: http://app.test/login\n   ```\n\n2. **Open the app as whoever**\n   ```browser\n   as: \"{{who}}\"\n   steps:\n     - goto: http://app.test/\n   ```\n";

    #[tokio::test(start_paused = true)]
    async fn logins_that_sign_in_as_each_other_through_templates_fail_at_once_naming_the_chain() {
        let f = fixture(
            Setup {
                refresh: "0s",
                params: r#"params = { who = "admin" }"#,
                logins_meet: 2,
                latency: Duration::from_secs(1),
                login: Some(OPEN_SIGN_IN_THEN_AS_WHO),
                config: "[auth.admin]\ntype = \"browser\"\nlogin = \"admin-sign-in\"\nparams = { who = \"web\" }",
                fragments: &[("admin-sign-in", OPEN_SIGN_IN_THEN_AS_WHO)],
                ..Setup::default()
            },
            token,
        );
        let started = tokio::time::Instant::now();
        let (web, admin) = tokio::time::timeout(Duration::from_secs(600), async { tokio::join!(f.token("web"), f.token("admin")) }).await.expect("neither waits forever");
        let (web, admin) = (web.err().expect("web fails"), admin.err().expect("admin fails"));
        for failed in [&web, &admin] {
            assert!(failed.contains("its login signs in as itself (web → admin → web)"), "{failed}");
        }
        assert_eq!(started.elapsed(), Duration::from_secs(1), "refused as soon as both logins reach their `as:` step");
        assert_eq!(f.browser.opened(), 2, "one login browser each");
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_login_that_names_its_own_profile_as_a_number_fails_without_opening_a_browser() {
        let f = fixture(
            Setup {
                refresh: "0s",
                login: Some("1. **Open the app as 7**\n   ```browser\n   as: 7\n   steps:\n     - goto: http://app.test/\n   ```\n"),
                config: "[auth.7]\ntype = \"browser\"\nlogin = \"sign-in\"",
                ..Setup::default()
            },
            token,
        );
        let failed = f.token("7").await.err().expect("the login is refused");
        assert!(failed.contains("(7 → 7)"), "{failed}");
        assert_eq!(f.browser.opens(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn first_uses_of_both_kinds_share_one_sign_in() {
        let (entered, gate) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        let f = refreshing("60s", gated(0, &entered, &gate, token));
        let uses = async {
            tokio::join!(f.engine.new_flow_session("web", &[]), f.token("web"), async {
                entered.notified().await;
                gate.notify_one();
            })
        };
        let (browser_use, token_use, ()) = tokio::time::timeout(Duration::from_secs(60), uses).await.expect("both are answered");
        assert_eq!(bearer(&token_use).as_deref(), Some("Bearer t0"));
        browser_use.expect("a browser signed in as `web`").close().await.expect("closes");
        assert_eq!((f.browser.evals(), f.browser.opened()), (1, 2), "one sign-in, then the flow's browser");
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_sign_in_answers_both_kinds_for_the_rest_of_the_run() {
        let f = fixture(Setup { refresh: "0s", login_fails: true, ..Setup::default() }, token);
        let failed = f.token("web").await.err().expect("the login fails");
        tokio::time::advance(Duration::from_secs(3600)).await;
        assert_eq!(f.token("web").await.err().as_deref(), Some(failed.as_str()));
        let browser = f.engine.new_flow_session("web", &[]).await.err().expect("no browser without a sign-in").to_string();
        assert!(browser.contains(&failed), "{browser}");
        assert_eq!(f.browser.opens(), 1, "the sign-in is not tried again");
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_fresh_token_is_kept_for_both_kinds() {
        let f = refreshing("60s", token);
        assert_eq!(bearer(&f.token("web").await).as_deref(), Some("Bearer t0"));
        tokio::time::advance(Duration::from_secs(59)).await;
        assert_eq!(bearer(&f.token("web").await).as_deref(), Some("Bearer t0"));
        f.engine.new_flow_session("web", &[]).await.expect("a browser signed in as `web`").close().await.expect("closes");
        assert_eq!(f.browser.evals(), 1, "nothing was refreshed");
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_sign_in_whose_browser_stops_answering_gives_up_for_every_use_waiting_on_it() {
        let f = fixture(Setup { refresh: "60s", hangs: &[Hang::SignInUrl], ..Setup::default() }, token);
        let started = tokio::time::Instant::now();
        let uses = async { tokio::join!(f.token("web"), f.engine.new_flow_session("web", &[])) };
        let (token_use, browser_use) = tokio::time::timeout(Duration::from_secs(600), uses).await.expect("the sign-in is bounded");
        assert_eq!(started.elapsed(), Duration::from_secs(60), "six step timeouts");
        let failed = token_use.err().expect("the sign-in gives up");
        assert!(failed.contains("signing in gave no answer within 60s"), "{failed}");
        let browser = browser_use.err().expect("the browser use gets the same failure").to_string();
        assert!(browser.contains(&failed), "{browser}");
        assert_eq!(f.browser.opened(), 1);
        f.browser.assert_each_closed_once().await;
    }

    /// Poll `waiting` once, so it is queued on the attempt in flight.
    async fn queue<T>(waiting: &mut Pin<Box<impl Future<Output = T>>>) {
        tokio::select! {
            biased;
            _ = waiting.as_mut() => panic!("it should wait on the attempt in flight"),
            () = std::future::ready(()) => {}
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_dropped_refresh_is_tried_again_by_a_use_waiting_on_it() {
        let (entered, gate) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        let f = refreshing("0s", gated(1, &entered, &gate, token));
        f.token("web").await.expect("signs in");
        let mut leading = Box::pin(f.token("web"));
        tokio::select! {
            _ = leading.as_mut() => panic!("the refresh is held at its token read"),
            () = entered.notified() => {}
        }
        let mut waiting = Box::pin(f.token("web"));
        queue(&mut waiting).await;
        drop(leading);
        let refreshed = tokio::time::timeout(Duration::from_secs(60), waiting).await.expect("the waiting use is answered");
        assert_eq!(bearer(&refreshed).as_deref(), Some("Bearer t2"));
        assert_eq!(f.browser.opened(), 3, "the sign-in, the dropped refresh and the new one");
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_dropped_sign_in_is_tried_again_by_a_use_waiting_on_it() {
        let (entered, gate) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        let f = refreshing("60s", gated(0, &entered, &gate, token));
        let mut leading = Box::pin(f.token("web"));
        tokio::select! {
            _ = leading.as_mut() => panic!("the sign-in is held at its token read"),
            () = entered.notified() => {}
        }
        let mut waiting = Box::pin(f.engine.new_flow_session("web", &[]));
        queue(&mut waiting).await;
        drop(leading);
        let browser = tokio::time::timeout(Duration::from_secs(60), waiting).await.expect("the waiting use is answered");
        browser.expect("a browser signed in as `web`").close().await.expect("closes");
        assert_eq!(bearer(&f.token("web").await).as_deref(), Some("Bearer t1"), "the sign-in it ran is kept");
        assert_eq!(f.browser.opened(), 3, "the dropped sign-in, the new one and the flow's browser");
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_refresh_whose_token_read_hangs_after_a_slow_commit_says_so() {
        let f = fixture(Setup { refresh: "0s", latency: Duration::from_secs(1), ..Setup::default() }, |n| if n == 0 { token(n) } else { Box::pin(std::future::pending()) });
        f.token("web").await.expect("signs in");
        let started = tokio::time::Instant::now();
        let failed = tokio::time::timeout(Duration::from_secs(600), f.token("web")).await.expect("bounded").err().expect("the token read gives up");
        assert!(failed.contains("fetching token from http://app.test/api/session gave no answer within 9s"), "{failed}");
        assert_eq!(started.elapsed(), Duration::from_secs(10), "the refresh's one step timeout");
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_sign_in_whose_storage_state_hangs_after_a_slow_login_says_so() {
        let f = fixture(Setup { refresh: "0s", latency: Duration::from_secs(55), hangs: &[Hang::SignInStorage], ..Setup::default() }, token);
        let started = tokio::time::Instant::now();
        let failed = tokio::time::timeout(Duration::from_secs(600), f.token("web")).await.expect("bounded").err().expect("the sign-in gives up");
        assert!(failed.contains("reading the signed-in browser's state gave no answer within 5s"), "{failed}");
        assert_eq!(started.elapsed(), Duration::from_secs(60), "what was left of the sign-in's six step timeouts");
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_sign_in_whose_browser_does_not_open_gives_up_and_closes_it_when_it_opens() {
        let f = fixture(Setup { refresh: "60s", hangs: &[Hang::OpenWithoutStorage], ..Setup::default() }, token);
        let started = tokio::time::Instant::now();
        let failed = tokio::time::timeout(Duration::from_secs(600), f.token("web")).await.expect("bounded").err().expect("the sign-in gives up");
        assert!(failed.contains("signing in gave no answer within 60s"), "{failed}");
        assert_eq!(started.elapsed(), Duration::from_secs(60));
        assert_eq!(f.browser.opened(), 0);
        let closed = f.browser.closing.notified();
        f.browser.release.notify_one();
        tokio::time::timeout(Duration::from_secs(60), closed).await.expect("the late browser is closed");
        assert_eq!(f.browser.opened(), 1);
        f.browser.assert_each_closed_once().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_flow_browser_that_opens_after_its_use_was_dropped_is_closed() {
        let f = fixture(Setup { refresh: "60s", hangs: &[Hang::OpenWithoutStorage], ..Setup::default() }, token);
        tokio::time::timeout(Duration::from_secs(1), f.engine.new_flow_session("", &[])).await.err().expect("the use is dropped while its browser opens");
        let closed = f.browser.closing.notified();
        f.browser.release.notify_one();
        tokio::time::timeout(Duration::from_secs(60), closed).await.expect("the late browser is closed");
        assert_eq!(f.browser.opened(), 1);
        f.browser.assert_each_closed_once().await;
    }
}
