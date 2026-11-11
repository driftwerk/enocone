//! Run flows: one isolated context per flow, steps in document order,
//! cleanup always, retries marked as flaky, evidence on every step.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use enocone_spec::blocks::{Block, parse_duration_str};
use enocone_spec::results::*;
use enocone_spec::template::{Resolver, render};
use enocone_spec::{BlockKind, Flow, SectionKind, Step};
use serde_json::Value;
use tokio::sync::{Mutex, Semaphore};

use crate::browser::{Timeouts, run_browser};
use crate::config::{AuthProfile, Project, Screenshots};
use crate::driver::{Driver, Session, SessionOptions};
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

#[derive(Clone)]
struct AuthState {
    headers: Vec<(String, String)>,
    storage: Option<Value>,
    /// When the token was read, for profiles with `refresh`.
    fetched: Instant,
}

pub struct Engine {
    pub project: Project,
    pub opts: RunOptions,
    driver: Option<Arc<dyn Driver>>,
    http: reqwest::Client,
    pub masker: Masker,
    env_vars: serde_json::Map<String, Value>,
    auth: Mutex<HashMap<String, Result<AuthState, String>>>,
    events: EventSink,
}

struct FlowState {
    vars: Vars,
    sessions: HashMap<String, Box<dyn Session>>,
    flow_id: String,
    attempt: u32,
    /// Only emails sent after this count.
    started: chrono::DateTime<chrono::Utc>,
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
        Ok(Engine { project, opts, driver, http, masker: Masker::default(), env_vars, auth: Mutex::new(HashMap::new()), events })
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
        self.auth(profile).await.map(|a| a.headers)
    }

    /// A browser session for exploration, optionally signed in as a profile.
    pub async fn open_session(&self, profile: Option<&str>) -> anyhow::Result<Box<dyn Session>> {
        let driver = self.driver.as_ref().ok_or_else(|| anyhow!("no browser driver is running"))?;
        let storage = match profile.filter(|p| !p.is_empty()) {
            None => None,
            Some(p) => {
                let a = self.auth(p).await.map_err(|e| anyhow!("auth profile `{p}`: {e}"))?;
                Some(a.storage.ok_or_else(|| anyhow!("auth profile `{p}` is not a browser login"))?)
            }
        };
        driver.new_session(SessionOptions { storage_state: storage }).await
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
                let _permit = sem.acquire_owned().await.expect("semaphore");
                (me.events)(Event::FlowStarted { id: flow.id.clone(), title: flow.title.clone() });
                let r = me.run_flow(flow).await;
                (me.events)(Event::FlowFinished(Box::new(r.clone())));
                r
            }));
        }
        let mut results = Vec::new();
        for h in handles {
            match h.await {
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
        let mut st = FlowState { vars: self.vars(n), sessions: HashMap::new(), flow_id: flow.id.clone(), attempt: n, started: chrono::Utc::now() };
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
        for (_, s) in st.sessions.drain() {
            let _ = s.close().await;
        }
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
                    Block::Http(spec) => {
                        let auth = match &spec.auth {
                            Some(name) => match self.auth(name).await {
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
                        if let Err(e) = self.ensure_session(st, &key).await {
                            r.error = Some(StepError { message: self.masker.mask(&e.to_string()), expected: None, actual: None });
                            break;
                        }
                        let session = st.sessions.get(&key).expect("session");
                        let t = Timeouts { action: self.opts.timeout, navigation: self.opts.navigation_timeout };
                        let out = run_browser(session.as_ref(), &spec, self.base_url().as_deref(), &t, &self.masker).await;
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

    async fn ensure_session(&self, st: &mut FlowState, profile: &str) -> anyhow::Result<()> {
        if st.sessions.contains_key(profile) {
            return Ok(());
        }
        let driver = self.driver.as_ref().ok_or_else(|| anyhow!("this step needs a browser, but no browser driver is running"))?;
        let storage = if profile.is_empty() {
            None
        } else {
            let a = self.auth(profile).await.map_err(|e| anyhow!("auth profile `{profile}`: {e}"))?;
            if a.storage.is_none() {
                bail!("auth profile `{profile}` is not a browser login, so it can't sign in a browser (use type = \"browser\")");
            }
            a.storage
        };
        let s = driver.new_session(SessionOptions { storage_state: storage }).await?;
        st.sessions.insert(profile.to_string(), s);
        Ok(())
    }

    /// Resolve an auth profile once per run (logins are serialised).
    async fn auth(&self, name: &str) -> Result<AuthState, String> {
        let mut cache = self.auth.lock().await;
        if let Some(r) = cache.get(name) {
            let stale = match (r, self.project.config.auth.get(name)) {
                (Ok(state), Some(AuthProfile::Browser { token: Some(t), .. })) => match t.refresh.as_deref().map(parse_duration_str) {
                    Some(Ok(age)) => state.fetched.elapsed() >= age,
                    _ => false,
                },
                _ => false,
            };
            if !stale {
                return r.clone();
            }
            let state = r.clone().expect("checked above");
            (self.events)(Event::Log(format!("refreshing the token for `{name}`")));
            let fresh = self.refresh_token(name, state).await.map_err(|e| self.masker.mask(&format!("{e:#}")));
            cache.insert(name.to_string(), fresh.clone());
            return fresh;
        }
        if matches!(self.project.config.auth.get(name), Some(AuthProfile::Browser { .. } | AuthProfile::Oauth2 { .. })) {
            (self.events)(Event::Log(format!("signing in as `{name}`")));
        }
        let r = self.resolve_auth(name).await.map_err(|e| self.masker.mask(&format!("{e:#}")));
        cache.insert(name.to_string(), r.clone());
        r
    }

    fn render_str(&self, s: &str) -> anyhow::Result<String> {
        let vars = self.vars(0);
        match enocone_spec::template::render_str(s, &vars).map_err(|e| anyhow!(e))? {
            Value::String(s) => Ok(s),
            other => Ok(value_preview(&other)),
        }
    }

    /// Read a browser profile's token again from its saved session (no new login).
    async fn refresh_token(&self, name: &str, state: AuthState) -> anyhow::Result<AuthState> {
        let Some(AuthProfile::Browser { token: Some(t), .. }) = self.project.config.auth.get(name).cloned() else { bail!("not a browser profile with a token") };
        let url = self.render_str(&t.url)?;
        let fail = |e: anyhow::Error| {
            anyhow!(
                "refreshing the token for `{name}` from {url} failed: {e:#}. The saved browser session may have expired too; sign-in runs once per run, so lengthen the session or shorten the suite"
            )
        };
        let driver = self.driver.as_ref().ok_or_else(|| anyhow!("refreshing needs a browser, but no browser driver is running"))?;
        let session = driver.new_session(SessionOptions { storage_state: state.storage.clone() }).await.map_err(fail)?;
        let result = async {
            // Any page on the token's origin will do for a same-origin fetch.
            let origin = reqwest::Url::parse(&url).map(|u| u.origin().ascii_serialization()).unwrap_or_else(|_| url.clone());
            session.goto(&origin, self.opts.navigation_timeout).await?;
            let tok = self.read_token(session.as_ref(), &url, &t.path).await?;
            let storage = session.storage_state().await.ok().or(state.storage.clone());
            anyhow::Ok(AuthState { headers: vec![("Authorization".to_string(), format!("Bearer {tok}"))], storage, fetched: Instant::now() })
        }
        .await;
        let _ = session.close().await;
        result.map_err(fail)
    }

    async fn read_token(&self, session: &dyn Session, url: &str, path: &str) -> anyhow::Result<String> {
        let js = format!("fetch({}, {{ credentials: 'include' }}).then(r => r.json())", serde_json::to_string(url)?);
        let body = session.eval(&js).await.with_context(|| format!("fetching token from {url}"))?;
        let path = if path.starts_with('$') { path.to_string() } else { format!("$.{path}") };
        let tok = json_at(&body, &path).and_then(|v| v.as_str().map(String::from)).ok_or_else(|| anyhow!("no token at {path} in {url} (are you signed in?)"))?;
        self.masker.add(&tok);
        Ok(tok)
    }

    async fn resolve_auth(&self, name: &str) -> anyhow::Result<AuthState> {
        let profile =
            self.project.config.auth.get(name).ok_or_else(|| anyhow!("not defined in enocone.toml (defined: {})", self.project.config.auth.keys().cloned().collect::<Vec<_>>().join(", ")))?.clone();
        match profile {
            AuthProfile::Header { header, value } => {
                let v = self.render_str(&value)?;
                self.masker.add(&v);
                Ok(AuthState { headers: vec![(header, v)], storage: None, fetched: Instant::now() })
            }
            AuthProfile::Bearer { token } => {
                let t = self.render_str(&token)?;
                self.masker.add(&t);
                Ok(AuthState { headers: vec![("Authorization".into(), format!("Bearer {t}"))], storage: None, fetched: Instant::now() })
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
                Ok(AuthState { headers: vec![("Authorization".into(), format!("Bearer {token}"))], storage: None, fetched: Instant::now() })
            }
            AuthProfile::Browser { login, params, token } => {
                let driver = self.driver.as_ref().ok_or_else(|| anyhow!("needs a browser to sign in, but no browser driver is running"))?;
                let fragment = self.project.load_fragment(&login)?;
                let session = driver.new_session(SessionOptions::default()).await?;
                let mut st = FlowState { vars: self.vars(0), sessions: HashMap::new(), flow_id: format!("auth-{name}"), attempt: 1, started: chrono::Utc::now() };
                for (k, v) in &params {
                    let v = self.render_str(v)?;
                    st.vars.set(k, Value::String(v));
                }
                st.sessions.insert(String::new(), session);
                let mut failure = None;
                for step in fragment.steps() {
                    let r = self.exec_step(&mut st, step, "").await;
                    if r.status == Status::Failed {
                        failure = Some(format!(
                            "login step \"{}\" failed: {}{}",
                            step.intent,
                            r.error.map(|e| e.message).unwrap_or_default(),
                            r.screenshot.map(|s| format!(" (screenshot: {s})")).unwrap_or_default()
                        ));
                        break;
                    }
                }
                let session = st.sessions.remove("").expect("login session");
                if let Some(f) = failure {
                    let _ = session.close().await;
                    bail!(f);
                }
                let storage = session.storage_state().await?;
                let mut headers = vec![];
                if let Some(t) = token {
                    let url = self.render_str(&t.url)?;
                    let tok = self.read_token(session.as_ref(), &url, &t.path).await?;
                    headers.push(("Authorization".to_string(), format!("Bearer {tok}")));
                }
                let _ = session.close().await;
                Ok(AuthState { headers, storage: Some(storage), fetched: Instant::now() })
            }
        }
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
