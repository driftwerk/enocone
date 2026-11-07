//! `{{ … }}` substitution, applied to block values at execution time.
//!
//! A string that is exactly one `{{expr}}` takes the variable's JSON value
//! (so numbers stay numbers); otherwise values are interpolated as text.

use serde_json::{Map, Value};

/// Looks up a dotted variable path such as `env.API`, `quote_id` or `lead.id`.
pub trait Resolver {
    fn lookup(&self, path: &str) -> Option<Value>;
    /// Names available, for error messages.
    fn known(&self) -> Vec<String> {
        vec![]
    }
}

/// Find a dotted path in a JSON value: `a.b.0.c`.
pub fn dig<'a>(root: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = root;
    for part in path {
        cur = match cur {
            Value::Object(m) => m.get(*part)?,
            Value::Array(a) => a.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn find_templates(s: &str) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(start) = s[i..].find("{{") {
        let start = i + start;
        let Some(end) = s[start + 2..].find("}}") else { break };
        let end = start + 2 + end;
        out.push((start, end + 2, s[start + 2..end].trim()));
        i = end + 2;
    }
    out
}

pub fn render_str(s: &str, r: &dyn Resolver) -> Result<Value, String> {
    let parts = find_templates(s);
    if parts.is_empty() {
        return Ok(Value::String(s.to_string()));
    }
    let lookup = |expr: &str| -> Result<Value, String> {
        // `{{ x | default: y }}`
        let (expr, default) = match expr.split_once('|') {
            Some((e, d)) => (e.trim(), d.trim().strip_prefix("default:").map(|d| d.trim().trim_matches('"').to_string())),
            None => (expr, None),
        };
        match r.lookup(expr) {
            Some(v) => Ok(v),
            None => match default {
                Some(d) => Ok(Value::String(d)),
                None => {
                    let known = r.known();
                    let hint = if known.is_empty() { String::new() } else { format!(" (known: {})", known.join(", ")) };
                    Err(format!("unknown variable `{expr}`{hint}"))
                }
            },
        }
    };
    if parts.len() == 1 && parts[0].0 == 0 && parts[0].1 == s.len() {
        return lookup(parts[0].2);
    }
    let mut out = String::new();
    let mut last = 0;
    for (start, end, expr) in parts {
        out.push_str(&s[last..start]);
        out.push_str(&crate::results::value_preview(&lookup(expr)?));
        last = end;
    }
    out.push_str(&s[last..]);
    Ok(Value::String(out))
}

pub fn render(v: &Value, r: &dyn Resolver) -> Result<Value, String> {
    Ok(match v {
        Value::String(s) => render_str(s, r)?,
        Value::Array(a) => Value::Array(a.iter().map(|x| render(x, r)).collect::<Result<_, _>>()?),
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, val) in m {
                let key = match render_str(k, r)? {
                    Value::String(s) => s,
                    other => crate::results::value_preview(&other),
                };
                out.insert(key, render(val, r)?);
            }
            Value::Object(out)
        }
        other => other.clone(),
    })
}

/// Every variable a value refers to (for validation and docs).
pub fn variables(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => out.extend(find_templates(s).into_iter().map(|(_, _, e)| e.split('|').next().unwrap().trim().to_string())),
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            Value::Object(m) => m.iter().for_each(|(k, x)| {
                walk(&Value::String(k.clone()), out);
                walk(x, out)
            }),
            _ => {}
        }
    }
    walk(v, &mut out);
    out
}

/// A simple resolver over a JSON object.
pub struct MapResolver<'a>(pub &'a Value);

impl Resolver for MapResolver<'_> {
    fn lookup(&self, path: &str) -> Option<Value> {
        let parts: Vec<&str> = path.split('.').collect();
        dig(self.0, &parts).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders() {
        let ctx = json!({ "env": { "API": "http://x" }, "total": 1368, "lead": { "id": "ld1" } });
        let r = MapResolver(&ctx);
        assert_eq!(render_str("{{env.API}}/leads/{{lead.id}}", &r).unwrap(), json!("http://x/leads/ld1"));
        assert_eq!(render_str("{{ total }}", &r).unwrap(), json!(1368));
        assert_eq!(render(&json!({ "a": ["{{total}}"] }), &r).unwrap(), json!({ "a": [1368] }));
        assert!(render_str("{{nope}}", &r).unwrap_err().contains("nope"));
        assert_eq!(render_str("{{nope | default: x}}", &r).unwrap(), json!("x"));
        assert_eq!(variables(&json!({"u": "{{env.API}}/{{id}}"})), vec!["env.API", "id"]);
    }
}
