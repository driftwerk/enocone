//! Variables a flow can use, and masking of secrets in everything recorded.

use std::sync::{Arc, Mutex};

use enocone_spec::template::{Resolver, dig};
use serde_json::{Map, Value};

/// Remembers secret values and hides them in recorded output.
#[derive(Default, Clone)]
pub struct Masker {
    secrets: Arc<Mutex<Vec<String>>>,
}

pub const MASK: &str = "••••";

impl Masker {
    pub fn add(&self, secret: &str) {
        if secret.len() < 4 {
            return;
        }
        let mut s = self.secrets.lock().unwrap();
        if !s.iter().any(|x| x == secret) {
            s.push(secret.to_string());
            // Longest first so overlapping secrets are fully hidden.
            s.sort_by_key(|x| std::cmp::Reverse(x.len()));
        }
    }
    pub fn mask(&self, text: &str) -> String {
        let mut out = text.to_string();
        for s in self.secrets.lock().unwrap().iter() {
            if out.contains(s.as_str()) {
                out = out.replace(s.as_str(), MASK);
            }
        }
        out
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
