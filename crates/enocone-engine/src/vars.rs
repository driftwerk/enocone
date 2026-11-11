//! Variables a flow can use, and masking of secrets in everything recorded.

use std::sync::{Arc, Mutex};

use enocone_spec::template::{Resolver, dig};
use serde_json::{Map, Value};

/// Remembers secret values and hides them in recorded output.
#[derive(Default, Clone)]
pub struct Masker {
    /// (value, what replaces it), longest first.
    secrets: Arc<Mutex<Vec<(String, String)>>>,
}

pub const MASK: &str = "••••";

impl Masker {
    pub fn add(&self, secret: &str) {
        self.add_as(secret, MASK);
    }

    /// A captured value marked secret: shown as `•••name•••`, so a reader can
    /// tell which value went where. Its URL-encoded forms, and the values of
    /// its own query parameters, are masked the same way.
    pub fn add_capture(&self, name: &str, secret: &str) {
        let label = format!("•••{name}•••");
        self.add_as(secret, &label);
        self.add_as(&url_encode(secret, false), &label);
        self.add_as(&url_encode(secret, true), &label);
        if let Some((_, query)) = secret.split_once('?') {
            for pair in query.split('#').next().unwrap_or("").split('&') {
                if let Some((_, v)) = pair.split_once('=')
                    && v.len() >= 8
                {
                    self.add_as(v, &label);
                    self.add_as(&url_encode(v, false), &label);
                }
            }
        }
    }

    fn add_as(&self, secret: &str, label: &str) {
        if secret.len() < 4 {
            return;
        }
        let mut s = self.secrets.lock().unwrap();
        if !s.iter().any(|(x, _)| x == secret) {
            s.push((secret.to_string(), label.to_string()));
            // Longest first so overlapping secrets are fully hidden.
            s.sort_by_key(|(x, _)| std::cmp::Reverse(x.len()));
        }
    }

    pub fn mask(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (s, label) in self.secrets.lock().unwrap().iter() {
            if out.contains(s.as_str()) {
                out = out.replace(s.as_str(), label);
            }
        }
        out
    }

    /// Mask every string in a recorded value (results, before they are written).
    pub fn scrub<T: serde::Serialize + serde::de::DeserializeOwned>(&self, v: T) -> T {
        if self.secrets.lock().unwrap().is_empty() {
            return v;
        }
        fn walk(m: &Masker, v: &mut Value) {
            match v {
                Value::String(s) => {
                    let masked = m.mask(s);
                    if masked != *s {
                        *s = masked;
                    }
                }
                Value::Array(a) => a.iter_mut().for_each(|x| walk(m, x)),
                Value::Object(o) => o.values_mut().for_each(|x| walk(m, x)),
                _ => {}
            }
        }
        // Fail closed: results that can't be masked must not be written unmasked.
        let mut j = serde_json::to_value(&v).expect("recorded results serialize to JSON");
        walk(self, &mut j);
        serde_json::from_value(j).expect("masked results read back as they were written")
    }

    pub fn mask_header(&self, name: &str, value: &str) -> String {
        let n = name.to_ascii_lowercase();
        if n == "authorization" || n == "cookie" || n == "set-cookie" || n.contains("api-key") || n.contains("token") || n.contains("secret") {
            match value.split_once(' ') {
                Some((scheme, _)) if n == "authorization" => format!("{scheme} {MASK}"),
                _ => MASK.to_string(),
            }
        } else {
            self.mask(value)
        }
    }
}

/// Everything `{{…}}` can refer to while a flow runs.
pub struct Vars {
    pub env: Map<String, Value>,
    pub run: Map<String, Value>,
    /// Values captured by steps (and fragment parameters).
    pub captured: Map<String, Value>,
    pub masker: Masker,
}

impl Vars {
    pub fn new(env: Map<String, Value>, run_id: &str, attempt: u32, masker: Masker) -> Self {
        // The end of the run id (minutes, seconds and a random part), not its date.
        let alnum: Vec<char> = run_id.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        let short = alnum[alnum.len().saturating_sub(8)..].iter().collect::<String>().to_lowercase();
        let mut run = Map::new();
        run.insert("id".into(), Value::String(run_id.to_string()));
        run.insert("short_id".into(), Value::String(short));
        run.insert("attempt".into(), Value::from(attempt));
        Vars { env, run, captured: Map::new(), masker }
    }

    pub fn set(&mut self, name: &str, v: Value) {
        self.captured.insert(name.to_string(), v);
    }
}

fn rand_hex(n: usize) -> String {
    uuid::Uuid::new_v4().simple().to_string()[..n].to_string()
}

impl Resolver for Vars {
    fn lookup(&self, path: &str) -> Option<Value> {
        let parts: Vec<&str> = path.split('.').map(str::trim).collect();
        match parts[0] {
            "env" if parts.len() >= 2 => dig(&Value::Object(self.env.clone()), &parts[1..]).cloned(),
            "secret" if parts.len() == 2 => {
                let v = std::env::var(parts[1]).ok()?;
                self.masker.add(&v);
                Some(Value::String(v))
            }
            "run" if parts.len() == 2 => self.run.get(parts[1]).cloned(),
            "fake" if parts.len() == 2 => Some(Value::String(match parts[1] {
                "email" => format!("enocone+{}@example.test", rand_hex(10)),
                "uuid" => uuid::Uuid::new_v4().to_string(),
                "name" => format!("Test Person {}", rand_hex(4)),
                "first_name" => "Test".into(),
                "last_name" => format!("Person{}", rand_hex(4)),
                "phone" => format!("07700 9{}", &uuid::Uuid::new_v4().as_u128().to_string()[..5]),
                "id" => rand_hex(12),
                _ => return None,
            })),
            "now" if parts.len() == 1 => Some(Value::String(chrono::Utc::now().to_rfc3339())),
            "today" if parts.len() == 1 => Some(Value::String(chrono::Utc::now().format("%Y-%m-%d").to_string())),
            _ => {
                let root = self.captured.get(parts[0])?;
                dig(root, &parts[1..]).cloned()
            }
        }
    }

    fn known(&self) -> Vec<String> {
        let mut k: Vec<String> = self.captured.keys().cloned().collect();
        k.extend(self.env.keys().map(|e| format!("env.{e}")));
        k.push("run.id".into());
        k
    }
}

/// Percent-encode everything but unreserved characters, as a URL inside
/// another URL's query is written (`lower` for lower-case hex).
fn url_encode(s: &str, lower: bool) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else if lower {
            out.push_str(&format!("%{b:02x}"));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_a_capture_and_its_encoded_forms() {
        let m = Masker::default();
        m.add_capture("invite_link", "http://app.test/accept?token=abcdef123456");
        assert_eq!(m.mask("open http://app.test/accept?token=abcdef123456 now"), "open •••invite_link••• now");
        assert_eq!(m.mask("/login?next=http%3A%2F%2Fapp.test%2Faccept%3Ftoken%3Dabcdef123456"), "/login?next=•••invite_link•••");
        assert_eq!(m.mask("/other?t=abcdef123456"), "/other?t=•••invite_link•••");
        let v: Value = m.scrub(serde_json::json!({ "a": ["x http://app.test/accept?token=abcdef123456"] }));
        assert_eq!(v["a"][0], "x •••invite_link•••");
    }

    #[test]
    fn scrub_keeps_everything_else_as_it_was() {
        use enocone_spec::results::{Attempt, FlowResult, RowResult, RunInfo, RunResults, Status, StepResult, Summary};
        let src = |p: &str| std::fs::read_to_string(format!("{}/../../e2e/fixtures/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let step = |status: Status, note: Option<&str>, rows: Vec<RowResult>| StepResult {
            key: "k".into(),
            status,
            duration_ms: 1,
            error: None,
            note: note.map(String::from),
            records: vec![],
            captured: vec![],
            screenshot: None,
            snapshot: None,
            page_url: None,
            console: vec![],
            children: vec![],
            rows,
        };
        let flows: Vec<FlowResult> =
            ["for-each/flows/setup/limit.flow.md", "conditions/flows/invites/pending.flow.md", "independent-tables/flows/access/callers.flow.md", "tables/flows/pricing/orders.flow.md"]
                .iter()
                .map(|p| {
                    let rows = vec![
                        RowResult { row: 1, table: 0, table_name: None, label: "a".into(), line: 3, result: step(Status::Passed, None, vec![]) },
                        RowResult { row: 2, table: 1, table_name: Some("refused".into()), label: "b".into(), line: 4, result: step(Status::Skipped, Some("skipped: only if x"), vec![]) },
                    ];
                    let attempts = vec![Attempt { status: Some(Status::Passed), steps: vec![step(Status::Passed, None, rows)], ..Default::default() }];
                    FlowResult { flow: enocone_spec::parse_flow(p, &src(p)).flow, status: Status::Passed, skip_reason: None, duration_ms: 1, attempts }
                })
                .collect();
        let results = RunResults { schema: enocone_spec::results::RESULTS_SCHEMA.into(), run: RunInfo::default(), summary: Summary::from_flows(&flows), flows, theme: None };
        let m = Masker::default();
        m.add_capture("never_seen", "a value that appears nowhere");
        let before = serde_json::to_value(&results).unwrap();
        let text = before.to_string();
        assert!(text.contains("\"for_each\"") && text.contains("\"only_if\"") && text.contains("\"parent\"") && text.contains("\"table_name\""));
        assert_eq!(serde_json::to_value(m.scrub(results)).unwrap(), before);
    }
}
