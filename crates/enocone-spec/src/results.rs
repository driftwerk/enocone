//! Run results (`results.json`, schema `enocone.results/v1`).
//!
//! A run carries the flow documents that actually ran, so a report can
//! always overlay results on the exact text, never on a later edit.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::Flow;

pub const RESULTS_SCHEMA: &str = "enocone.results/v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Failed,
    Flaky,
    Passed,
    Skipped,
    /// Agreed behaviour that is not implemented yet (no block, or the flow is a draft).
    Pending,
    /// Documentation view: nothing ran.
    #[serde(rename = "not-run")]
    NotRun,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Flaky => "flaky",
            Self::Skipped => "skipped",
            Self::Pending => "pending",
            Self::NotRun => "not run",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResults {
    pub schema: String,
    pub run: RunInfo,
    pub summary: Summary,
    pub flows: Vec<FlowResult>,
    /// The project's theme as CSS (variables from `theme/`), for drawing mockups.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunInfo {
    pub id: String,
    pub project: String,
    pub env: String,
    pub driver: String,
    pub started_at: String,
    pub duration_ms: u64,
    pub enocone_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci_url: Option<String>,
    /// `false` for the documentation view.
    #[serde(default = "yes")]
    pub executed: bool,
    /// Ran against the prototype (screens and storyboards), not the real app.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub prototype: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub flaky: usize,
    pub skipped: usize,
    #[serde(default)]
    pub pending: usize,
    pub steps: usize,
}

impl Summary {
    pub fn from_flows(flows: &[FlowResult]) -> Self {
        let mut s = Summary { total: flows.len(), ..Default::default() };
        for f in flows {
            match f.status {
                Status::Passed => s.passed += 1,
                Status::Failed => s.failed += 1,
                Status::Flaky => s.flaky += 1,
                Status::Pending => s.pending += 1,
                Status::Skipped | Status::NotRun => s.skipped += 1,
            }
            if let Some(a) = f.attempts.last() {
                s.steps += a.steps.iter().filter(|st| matches!(st.status, Status::Passed | Status::Failed)).count();
            }
        }
        s
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowResult {
    /// Snapshot of the document that ran.
    pub flow: Flow,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
    pub duration_ms: u64,
    /// Every attempt; the last one is shown by default.
    pub attempts: Vec<Attempt>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Attempt {
    pub status: Option<Status>,
    pub duration_ms: u64,
    pub steps: Vec<StepResult>,
    /// Values captured during the attempt (secrets masked).
    #[serde(default)]
    pub captured: Vec<(String, String)>,
    /// Paths of extra evidence (relative to the report), e.g. a trace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub label: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub key: String,
    pub status: Status,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<StepError>,
    /// Why it was skipped, when it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<Record>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub captured: Vec<(String, String)>,
    /// Screenshot taken after the step (path relative to the report).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<String>,
    /// Accessibility snapshot of the page, kept on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub console: Vec<String>,
    /// Steps of an included fragment (`use`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ChildStep>,
    /// One result per row of the step's table of examples.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<RowResult>,
}

/// How one row of a table of examples went.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowResult {
    /// 1-based, in the table's order.
    pub row: usize,
    /// Which of the step's tables the row is in (0, the first, when left out).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub table: usize,
    /// That table's name, when the step runs several tables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_name: Option<String>,
    /// The row's inputs, e.g. "returning, 120".
    pub label: String,
    /// Its line in the flow file.
    pub line: usize,
    pub result: StepResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildStep {
    pub intent: String,
    pub result: StepResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepError {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
}

/// What happened while a block ran.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Record {
    Http(HttpRecord),
    Action(ActionRecord),
    Exec(ExecRecord),
    Email(EmailRecord),
}

/// The message an email block found (or, when none came, what the mailbox held).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailRecord {
    pub to: String,
    pub mailbox: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub received_at: Option<String>,
    /// The text body (or HTML with tags removed), shortened.
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
    /// When nothing matched: the messages the mailbox did have for that address.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen: Vec<String>,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecRecord {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRecord {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub request_headers: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default)]
    pub response_headers: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_body: Option<String>,
    pub duration_ms: u64,
    /// For `eventually`: each poll's status and a short summary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub polls: Vec<Poll>,
    /// Assertion outcomes, e.g. `$.status = "paid" ✓`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Poll {
    pub at_ms: u64,
    pub status: Option<u16>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub what: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRecord {
    pub description: String,
    pub ok: bool,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RunResults {
    /// 0 = all good, 1 = failures (or pending steps when `fail_on_pending`).
    pub fn exit_code(&self, fail_on_pending: bool) -> i32 {
        if self.summary.failed > 0 || (fail_on_pending && self.summary.pending > 0) { 1 } else { 0 }
    }
    /// Documentation view: the flows with nothing run.
    pub fn docs(project: &str, flows: Vec<Flow>) -> Self {
        let flows: Vec<FlowResult> = flows.into_iter().map(|flow| FlowResult { flow, status: Status::NotRun, skip_reason: None, duration_ms: 0, attempts: vec![] }).collect();
        RunResults {
            schema: RESULTS_SCHEMA.into(),
            run: RunInfo { project: project.into(), executed: false, prototype: false, enocone_version: env!("CARGO_PKG_VERSION").into(), ..Default::default() },
            summary: Summary { total: flows.len(), ..Default::default() },
            flows,
            theme: None,
        }
    }
}

pub fn value_preview(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}
