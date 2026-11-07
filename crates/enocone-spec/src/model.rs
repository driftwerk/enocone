//! The flow document model: what a `.flow.md` file parses into.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One flow document (`*.flow.md`), or a fragment included by `use`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Flow {
    /// Stable id, from frontmatter `id` or derived from the file path.
    pub id: String,
    /// The `# Title`: the business outcome.
    pub title: String,
    /// Markdown paragraphs under the title: why the flow matters.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub why: String,
    #[serde(default)]
    pub meta: FlowMeta,
    pub sections: Vec<Section>,
    /// Items under `## Open questions`: what reviewers still need to decide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<String>,
    /// Other `##` sections (rules, edge cases, notes), as Markdown, for the docs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<NoteSection>,
    /// Flow-level designs: flow charts, storyboards, screens outside steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub designs: Vec<crate::design::Design>,
    /// Example values for the prototype (`sample:` in the frontmatter).
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub sample: serde_json::Map<String, Value>,
    /// Path relative to the project root, with forward slashes.
    pub path: String,
    /// sha256 of the source text, so results can prove which text ran.
    pub source_hash: String,
    /// The full source as it was parsed.
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NoteSection {
    pub heading: String,
    pub markdown: String,
}

/// Where a flow is in its life: designed → reviewed → approved → implemented.
///
/// Greenfield work starts at `draft` (sentences only, no blocks) and moves
/// forward as people review the document and the AI implements it.
/// Backfilled flows for an existing system usually start at `implemented`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// Being written; behaviour still open for discussion. Not run.
    Draft,
    /// Ready for people to review. Not run.
    Review,
    /// Behaviour signed off; may be partly implemented. Runs what exists; the rest is pending.
    Approved,
    /// Fully implemented; every step must have a block.
    #[default]
    Implemented,
}

impl Stage {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "draft" | "proposed" | "design" => Some(Self::Draft),
            "review" | "in-review" | "in review" => Some(Self::Review),
            "approved" | "accepted" | "ready" => Some(Self::Approved),
            "implemented" | "live" | "done" => Some(Self::Implemented),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Review => "review",
            Self::Approved => "approved",
            Self::Implemented => "implemented",
        }
    }
    /// Whether `enocone run` executes this flow.
    pub fn runs(self) -> bool {
        matches!(self, Self::Approved | Self::Implemented)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FlowMeta {
    #[serde(default)]
    pub stage: Stage,
    /// Who approved the behaviour (free text, e.g. names or a PR link).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    /// Environments this flow may run against; empty means all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub envs: Vec<String>,
    /// Variables that must be set (e.g. `env.webhook_sink`); the flow is skipped otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    /// Set to true to skip the flow without deleting it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip: bool,
    /// Other frontmatter keys, kept for display.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum SectionKind {
    Given,
    When,
    Then,
    Cleanup,
}

impl SectionKind {
    pub fn parse(heading: &str) -> Option<Self> {
        match heading.trim().to_ascii_lowercase().as_str() {
            "given" | "background" | "setup" => Some(Self::Given),
            "when" => Some(Self::When),
            "then" => Some(Self::Then),
            "cleanup" | "clean up" | "teardown" | "finally" => Some(Self::Cleanup),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Given => "Given",
            Self::When => "When",
            Self::Then => "Then",
            Self::Cleanup => "Cleanup",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Section {
    pub kind: SectionKind,
    pub steps: Vec<Step>,
}

/// One step: a business sentence plus the blocks that perform it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Step {
    /// Stable key derived from the intent, so results line up across renumbering.
    pub key: String,
    /// 1-based number across Given/When/Then; `None` for cleanup steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    /// The sentence people read.
    pub intent: String,
    /// Extra prose under the step (documentation only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    pub blocks: Vec<RawBlock>,
    /// Mockups and diagrams for this step (documentation; never executed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub designs: Vec<crate::design::Design>,
    /// 1-based line of the step in the source.
    pub line: usize,
}

/// A fenced block as written. Interpreted (after templating) by [`crate::blocks`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawBlock {
    /// Info string: `http`, `browser`, `expect` or `use`.
    pub kind: BlockKind,
    /// The YAML text exactly as written.
    pub source: String,
    /// The YAML parsed to JSON (templates still unresolved).
    pub value: Value,
    /// 1-based line of the opening fence.
    pub line: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BlockKind {
    Http,
    Browser,
    Expect,
    Use,
    /// Run a command (needs `run.allow_exec = true`).
    Exec,
    /// Wait for an email in the project's mailbox and check or capture from it.
    Email,
}

impl BlockKind {
    pub fn parse(info: &str) -> Option<Self> {
        match info.split_whitespace().next().unwrap_or("") {
            "http" | "api" => Some(Self::Http),
            "browser" | "ui" => Some(Self::Browser),
            "expect" => Some(Self::Expect),
            "use" => Some(Self::Use),
            "exec" | "shell" => Some(Self::Exec),
            "email" | "mail" => Some(Self::Email),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Browser => "browser",
            Self::Expect => "expect",
            Self::Use => "use",
            Self::Exec => "exec",
            Self::Email => "email",
        }
    }
}

impl Flow {
    pub fn steps(&self) -> impl Iterator<Item = &Step> {
        self.sections.iter().flat_map(|s| s.steps.iter())
    }
    pub fn area(&self) -> &str {
        self.meta.area.as_deref().unwrap_or("General")
    }
    /// Steps that have no block yet: behaviour agreed, implementation to come.
    pub fn unimplemented_steps(&self) -> usize {
        self.steps().filter(|s| s.blocks.is_empty()).count()
    }
    pub fn uses_browser(&self) -> bool {
        self.steps().any(|s| s.blocks.iter().any(|b| matches!(b.kind, BlockKind::Browser | BlockKind::Expect)))
    }
}

/// Make a stable, URL-safe key from a sentence.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
        if out.len() >= 60 {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() { "step".into() } else { out }
}

impl Flow {
    /// Every screen in the flow (steps first, then flow-level), with the step key it belongs to.
    pub fn screens(&self) -> impl Iterator<Item = (Option<&Step>, &crate::design::Screen)> {
        self.steps().flat_map(|st| st.designs.iter().filter_map(move |d| d.screen.as_ref().map(|s| (Some(st), s)))).chain(self.designs.iter().filter_map(|d| d.screen.as_ref().map(|s| (None, s))))
    }
    pub fn screens_mut(&mut self) -> Vec<&mut crate::design::Screen> {
        let mut out: Vec<&mut crate::design::Screen> = vec![];
        for sec in self.sections.iter_mut() {
            for st in sec.steps.iter_mut() {
                for d in st.designs.iter_mut() {
                    if let Some(s) = d.screen.as_mut() {
                        out.push(s);
                    }
                }
            }
        }
        for d in self.designs.iter_mut() {
            if let Some(s) = d.screen.as_mut() {
                out.push(s);
            }
        }
        out
    }
}
