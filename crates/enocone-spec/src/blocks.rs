//! Typed meaning of the fenced blocks. Built from the YAML value, either
//! before templating (validation) or after (execution).

use std::time::Duration;

use serde_json::{Map, Value};

use crate::model::BlockKind;

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Block {
    Http(HttpSpec),
    Browser(BrowserSpec),
    Use(UseSpec),
    Exec(ExecSpec),
    Email(EmailSpec),
}

/// Take `secret: true` out of a block's captures (top-level `capture:`, or a
/// browser block's `- capture:` actions), returning the names marked secret.
pub fn take_secret_captures(v: &mut Value) -> Vec<String> {
    fn from_map(c: &mut Value, out: &mut Vec<String>) {
        let Some(m) = c.as_object_mut() else { return };
        for (name, spec) in m.iter_mut() {
            if let Some(o) = spec.as_object_mut()
                && let Some(s) = o.remove("secret")
                && s == Value::Bool(true)
            {
                out.push(name.clone());
            }
        }
    }
    let mut out = Vec::new();
    match v {
        Value::Object(m) => {
            if let Some(c) = m.get_mut("capture") {
                from_map(c, &mut out);
            }
            if let Some(Value::Array(actions)) = m.get_mut("steps") {
                for a in actions {
                    if let Some(c) = a.get_mut("capture") {
                        from_map(c, &mut out);
                    }
                }
            }
        }
        Value::Array(actions) => {
            for a in actions {
                if let Some(c) = a.get_mut("capture") {
                    from_map(c, &mut out);
                }
            }
        }
        _ => {}
    }
    out
}

impl Block {
    pub fn from_raw(kind: BlockKind, v: &Value) -> Result<Block, String> {
        let mut v = v.clone();
        take_secret_captures(&mut v);
        let v = &v;
        match kind {
            BlockKind::Http => HttpSpec::from_value(v).map(Block::Http),
            BlockKind::Browser => BrowserSpec::from_value(v).map(Block::Browser),
            BlockKind::Expect => {
                let exp = BrowserExpect::from_value(v)?;
                Ok(Block::Browser(BrowserSpec { as_profile: None, actions: vec![Action::Expect(exp)] }))
            }
            BlockKind::Use => UseSpec::from_value(v).map(Block::Use),
            BlockKind::Exec => ExecSpec::from_value(v).map(Block::Exec),
            BlockKind::Email => EmailSpec::from_value(v).map(Block::Email),
        }
    }
}

// ---------------------------------------------------------------- helpers

pub fn parse_duration(v: &Value) -> Result<Duration, String> {
    match v {
        Value::Number(n) => Ok(Duration::from_millis((n.as_f64().unwrap_or(0.0) * 1000.0) as u64)),
        Value::String(s) => parse_duration_str(s),
        _ => Err(format!("expected a duration like 10s or 500ms, got {v}")),
    }
}

pub fn parse_duration_str(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let (num, mult) = if let Some(n) = s.strip_suffix("ms") {
        (n, 1.0)
    } else if let Some(n) = s.strip_suffix('s') {
        (n, 1000.0)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 60_000.0)
    } else {
        (s, 1000.0)
    };
    num.trim().parse::<f64>().map(|n| Duration::from_millis((n * mult) as u64)).map_err(|_| format!("expected a duration like 10s or 500ms, got \"{s}\""))
}

fn as_str(v: &Value, what: &str) -> Result<String, String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        _ => Err(format!("{what} must be text, got {v}")),
    }
}

fn str_pairs(v: &Value, what: &str) -> Result<Vec<(String, String)>, String> {
    match v {
        Value::Object(m) => m.iter().map(|(k, v)| Ok((k.clone(), as_str(v, what)?))).collect(),
        Value::Null => Ok(vec![]),
        _ => Err(format!("{what} must be a mapping")),
    }
}

fn obj<'a>(v: &'a Value, what: &str) -> Result<&'a Map<String, Value>, String> {
    v.as_object().ok_or_else(|| format!("{what} must be a mapping, got {v}"))
}

fn check_keys(m: &Map<String, Value>, allowed: &[&str], what: &str) -> Result<(), String> {
    for k in m.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(format!("unknown key `{k}` in {what} (allowed: {})", allowed.join(", ")));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- matchers

/// How a value is checked. A plain value means equality.
#[derive(Debug, Clone, PartialEq)]
pub enum Matcher {
    Eq(Value),
    Exists(bool),
    Contains(Value),
    Matches(String),
    Gt(f64),
    Gte(f64),
    Lt(f64),
    Lte(f64),
    Len(usize),
    OneOf(Vec<Value>),
    Not(Box<Matcher>),
    All(Vec<Matcher>),
}

impl Matcher {
    /// Whether this checks a length, which for a path that can match several
    /// nodes means how many matched (RFC 9535: such a path yields a node list).
    pub fn counts(&self) -> bool {
        match self {
            Matcher::Len(_) => true,
            Matcher::Not(m) => m.counts(),
            Matcher::All(ms) => ms.iter().any(Matcher::counts),
            _ => false,
        }
    }
}

const MATCHER_OPS: &[&str] = &["exists", "contains", "matches", "gt", "gte", "lt", "lte", "len", "one_of", "not", "eq"];

impl Matcher {
    pub fn from_value(v: &Value) -> Result<Matcher, String> {
        if let Value::Object(m) = v
            && !m.is_empty()
            && m.keys().all(|k| MATCHER_OPS.contains(&k.as_str()))
        {
            let mut all = Vec::new();
            for (k, val) in m {
                let num = || val.as_f64().ok_or_else(|| format!("`{k}` needs a number"));
                all.push(match k.as_str() {
                    "eq" => Matcher::Eq(val.clone()),
                    "exists" => Matcher::Exists(val.as_bool().unwrap_or(true)),
                    "contains" => Matcher::Contains(val.clone()),
                    "matches" => {
                        let s = as_str(val, "matches")?;
                        regex::Regex::new(&s).map_err(|e| format!("bad regex: {e}"))?;
                        Matcher::Matches(s)
                    }
                    "gt" => Matcher::Gt(num()?),
                    "gte" => Matcher::Gte(num()?),
                    "lt" => Matcher::Lt(num()?),
                    "lte" => Matcher::Lte(num()?),
                    "len" => Matcher::Len(val.as_u64().ok_or("`len` needs a whole number")? as usize),
                    "one_of" => Matcher::OneOf(val.as_array().cloned().ok_or("`one_of` needs a list")?),
                    "not" => Matcher::Not(Box::new(Matcher::from_value(val)?)),
                    _ => unreachable!(),
                });
            }
            return Ok(if all.len() == 1 { all.pop().unwrap() } else { Matcher::All(all) });
        }
        Ok(Matcher::Eq(v.clone()))
    }

    /// Check an actual value (None = missing). Returns a description of the mismatch.
    pub fn check(&self, actual: Option<&Value>) -> Result<(), String> {
        let show = |v: Option<&Value>| v.map(|v| v.to_string()).unwrap_or_else(|| "nothing".into());
        match self {
            Matcher::Exists(want) => {
                let has = actual.is_some_and(|v| !v.is_null());
                if has == *want {
                    Ok(())
                } else if *want {
                    Err("expected a value, found nothing".into())
                } else {
                    Err(format!("expected nothing, found {}", show(actual)))
                }
            }
            Matcher::Eq(want) => {
                let Some(a) = actual else { return Err(format!("expected {want}, found nothing")) };
                if loose_eq(want, a) { Ok(()) } else { Err(format!("expected {want}, got {a}")) }
            }
            Matcher::Contains(want) => {
                let Some(a) = actual else { return Err(format!("expected something containing {want}, found nothing")) };
                let ok = match (a, want) {
                    (Value::String(s), w) => s.contains(&crate::results::value_preview(w)),
                    (Value::Array(items), w) => items.iter().any(|i| loose_eq(w, i)),
                    (Value::Object(m), Value::Object(w)) => w.iter().all(|(k, v)| m.get(k).is_some_and(|x| loose_eq(v, x))),
                    _ => false,
                };
                if ok { Ok(()) } else { Err(format!("expected {a} to contain {want}")) }
            }
            Matcher::Matches(re) => {
                let a = actual.map(crate::results::value_preview).unwrap_or_default();
                let r = regex::Regex::new(re).map_err(|e| e.to_string())?;
                if r.is_match(&a) { Ok(()) } else { Err(format!("expected {a:?} to match /{re}/")) }
            }
            Matcher::Gt(n) | Matcher::Gte(n) | Matcher::Lt(n) | Matcher::Lte(n) => {
                let a = actual.and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
                let Some(a) = a else { return Err(format!("expected a number, got {}", show(actual))) };
                let (ok, op) = match self {
                    Matcher::Gt(_) => (a > *n, ">"),
                    Matcher::Gte(_) => (a >= *n, ">="),
                    Matcher::Lt(_) => (a < *n, "<"),
                    _ => (a <= *n, "<="),
                };
                if ok { Ok(()) } else { Err(format!("expected {op} {n}, got {a}")) }
            }
            Matcher::Len(n) => {
                let len = match actual {
                    Some(Value::Array(a)) => a.len(),
                    Some(Value::String(s)) => s.chars().count(),
                    Some(Value::Object(m)) => m.len(),
                    _ => return Err(format!("expected something with length {n}, got {}", show(actual))),
                };
                if len == *n { Ok(()) } else { Err(format!("expected length {n}, got {len}")) }
            }
            Matcher::OneOf(opts) => {
                let Some(a) = actual else { return Err("expected a value, found nothing".into()) };
                if opts.iter().any(|o| loose_eq(o, a)) { Ok(()) } else { Err(format!("expected one of {}, got {a}", Value::Array(opts.clone()))) }
            }
            Matcher::Not(inner) => match inner.check(actual) {
                Ok(()) => Err(format!("expected not {}", inner.describe())),
                Err(_) => Ok(()),
            },
            Matcher::All(ms) => ms.iter().try_for_each(|m| m.check(actual)),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Matcher::Eq(v) => format!("= {v}"),
            Matcher::Exists(true) => "exists".into(),
            Matcher::Exists(false) => "is absent".into(),
            Matcher::Contains(v) => format!("contains {v}"),
            Matcher::Matches(r) => format!("matches /{r}/"),
            Matcher::Gt(n) => format!("> {n}"),
            Matcher::Gte(n) => format!(">= {n}"),
            Matcher::Lt(n) => format!("< {n}"),
            Matcher::Lte(n) => format!("<= {n}"),
            Matcher::Len(n) => format!("has length {n}"),
            Matcher::OneOf(v) => format!("is one of {}", Value::Array(v.clone())),
            Matcher::Not(m) => format!("not {}", m.describe()),
            Matcher::All(ms) => ms.iter().map(|m| m.describe()).collect::<Vec<_>>().join(" and "),
        }
    }
}

/// Equality that treats `"200"` and `200`, `"true"` and `true` alike, so
/// templated values compare sensibly.
pub fn loose_eq(want: &Value, actual: &Value) -> bool {
    if want == actual {
        return true;
    }
    match (want, actual) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::String(s), Value::Number(n)) | (Value::Number(n), Value::String(s)) => s.parse::<f64>().ok() == n.as_f64(),
        (Value::String(s), Value::Bool(b)) | (Value::Bool(b), Value::String(s)) => s == &b.to_string(),
        (Value::Object(w), Value::Object(a)) => w.len() == a.len() && w.iter().all(|(k, v)| a.get(k).is_some_and(|x| loose_eq(v, x))),
        (Value::Array(w), Value::Array(a)) => w.len() == a.len() && w.iter().zip(a).all(|(x, y)| loose_eq(x, y)),
        _ => false,
    }
}

// ---------------------------------------------------------------- http

pub const HTTP_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

#[derive(Debug, Clone, PartialEq)]
pub enum HttpBody {
    Json(Value),
    Form(Vec<(String, String)>),
    Text(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpSpec {
    pub method: String,
    pub url: String,
    pub auth: Option<String>,
    pub headers: Vec<(String, String)>,
    pub query: Vec<(String, String)>,
    pub body: Option<HttpBody>,
    pub expect: HttpExpect,
    pub capture: Vec<(String, HttpCapture)>,
    pub eventually: Option<Duration>,
    pub timeout: Option<Duration>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HttpExpect {
    /// Allowed statuses. Empty = any 2xx.
    pub status: Vec<u16>,
    pub json: Vec<(String, Matcher)>,
    pub headers: Vec<(String, Matcher)>,
    pub body: Option<Matcher>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HttpCapture {
    Json(String),
    Header(String),
    Regex(String),
    Status,
}

impl HttpSpec {
    pub fn from_value(v: &Value) -> Result<Self, String> {
        let m = obj(v, "an http block")?;
        let methods: Vec<&String> = m.keys().filter(|k| HTTP_METHODS.contains(&k.to_ascii_uppercase().as_str())).collect();
        let method = match methods.as_slice() {
            [one] => one.to_string(),
            [] => return Err("needs a method and URL, e.g. `GET: \"{{env.API}}/leads\"`".into()),
            _ => return Err("has more than one method; use one request per block".into()),
        };
        let url = as_str(&m[method.as_str()], "the URL")?;
        check_keys(m, &[method.as_str(), "auth", "headers", "query", "json", "form", "body", "expect", "capture", "eventually", "timeout"], "an http block")?;
        let body = match (m.get("json"), m.get("form"), m.get("body")) {
            (Some(j), None, None) => Some(HttpBody::Json(j.clone())),
            (None, Some(f), None) => Some(HttpBody::Form(str_pairs(f, "form")?)),
            (None, None, Some(b)) => Some(HttpBody::Text(as_str(b, "body")?)),
            (None, None, None) => None,
            _ => return Err("use only one of json, form or body".into()),
        };
        let expect = match m.get("expect") {
            Some(e) => HttpExpect::from_value(e)?,
            None => HttpExpect::default(),
        };
        let capture = match m.get("capture") {
            Some(c) => obj(c, "capture")?.iter().map(|(k, v)| Ok((k.clone(), HttpCapture::from_value(v)?))).collect::<Result<Vec<_>, String>>()?,
            None => vec![],
        };
        Ok(HttpSpec {
            method: method.to_ascii_uppercase(),
            url,
            auth: m.get("auth").map(|a| as_str(a, "auth")).transpose()?,
            headers: str_pairs(m.get("headers").unwrap_or(&Value::Null), "headers")?,
            query: str_pairs(m.get("query").unwrap_or(&Value::Null), "query")?,
            body,
            expect,
            capture,
            eventually: m.get("eventually").map(parse_duration).transpose()?,
            timeout: m.get("timeout").map(parse_duration).transpose()?,
        })
    }
}

impl HttpExpect {
    fn from_value(v: &Value) -> Result<Self, String> {
        let m = obj(v, "expect")?;
        check_keys(m, &["status", "json", "headers", "body"], "http expect")?;
        let status = match m.get("status") {
            None => vec![],
            Some(s) => statuses(s)?,
        };
        let json = match m.get("json") {
            None => vec![],
            Some(j) => obj(j, "expect.json")?
                .iter()
                .map(|(k, v)| {
                    let path = if k.starts_with('$') { k.clone() } else { format!("$.{k}") };
                    serde_json_path::JsonPath::parse(&path).map_err(|e| format!("bad JSONPath `{k}`: {e}"))?;
                    Ok((path, Matcher::from_value(v)?))
                })
                .collect::<Result<_, String>>()?,
        };
        let headers = match m.get("headers") {
            None => vec![],
            Some(h) => obj(h, "expect.headers")?.iter().map(|(k, v)| Ok((k.to_ascii_lowercase(), Matcher::from_value(v)?))).collect::<Result<_, String>>()?,
        };
        let body = match m.get("body") {
            None => None,
            Some(Value::String(s)) => Some(Matcher::Contains(Value::String(s.clone()))),
            Some(other) => Some(Matcher::from_value(other)?),
        };
        Ok(HttpExpect { status, json, headers, body })
    }
}

/// A value still holding a `{{…}}` template: filled in per row when the step runs.
fn templated(v: &Value) -> bool {
    matches!(v, Value::String(s) if s.contains("{{"))
}

/// Expected statuses: `200`, `[200, 201]` or `"200, 201"` (a table cell).
fn statuses(v: &Value) -> Result<Vec<u16>, String> {
    match v {
        _ if templated(v) => Ok(vec![]),
        Value::Array(a) => a.iter().map(status_code).collect(),
        Value::String(s) if s.contains(',') => s.split(',').map(|p| status_code(&Value::String(p.trim().into()))).collect(),
        _ => Ok(vec![status_code(v)?]),
    }
}

fn status_code(v: &Value) -> Result<u16, String> {
    let bad = || format!("status {v} isn't a status code: write one such as 200, or a list such as [200, 201]");
    let n = match v {
        Value::Number(n) => n.as_u64().ok_or_else(bad)?,
        Value::String(s) => s.trim().parse().map_err(|_| bad())?,
        _ => return Err(bad()),
    };
    if (100..600).contains(&n) { Ok(n as u16) } else { Err(bad()) }
}

/// Expected exit codes: `0`, `[0, 1]`, `"0, 1"` (a table cell) or `"any"`.
fn exit_codes(v: &Value) -> Result<Option<Vec<i32>>, String> {
    let bad = || format!("exit {v} isn't an exit code: write a number such as 0, a list such as [0, 1], or \"any\"");
    let one = |x: &Value| -> Result<i32, String> {
        match x {
            Value::Number(n) => n.as_i64().map(|n| n as i32).ok_or_else(bad),
            Value::String(s) => s.trim().parse().map_err(|_| bad()),
            _ => Err(bad()),
        }
    };
    match v {
        _ if templated(v) => Ok(Some(vec![0])),
        Value::String(s) if s == "any" => Ok(None),
        Value::Array(a) => a.iter().map(one).collect::<Result<_, _>>().map(Some),
        Value::String(s) if s.contains(',') => s.split(',').map(|p| one(&Value::String(p.into()))).collect::<Result<_, _>>().map(Some),
        _ => Ok(Some(vec![one(v)?])),
    }
}

impl HttpCapture {
    fn from_value(v: &Value) -> Result<Self, String> {
        match v {
            Value::String(s) => {
                let path = if s.starts_with('$') { s.clone() } else { format!("$.{s}") };
                serde_json_path::JsonPath::parse(&path).map_err(|e| format!("bad JSONPath `{s}`: {e}"))?;
                Ok(HttpCapture::Json(path))
            }
            Value::Object(m) => {
                if let Some(h) = m.get("header") {
                    Ok(HttpCapture::Header(as_str(h, "header")?.to_ascii_lowercase()))
                } else if let Some(r) = m.get("regex") {
                    let r = as_str(r, "regex")?;
                    regex::Regex::new(&r).map_err(|e| format!("bad regex: {e}"))?;
                    Ok(HttpCapture::Regex(r))
                } else if let Some(j) = m.get("json") {
                    HttpCapture::from_value(j)
                } else if m.contains_key("status") {
                    Ok(HttpCapture::Status)
                } else {
                    Err("capture needs a JSONPath like `$.id`, or {header: name}, {regex: …} or {status: true}".into())
                }
            }
            _ => Err(format!("bad capture {v}")),
        }
    }
}

// ---------------------------------------------------------------- browser

#[derive(Debug, Clone, PartialEq)]
pub struct BrowserSpec {
    /// Auth profile whose signed-in browser session to use.
    pub as_profile: Option<String>,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Goto(String),
    Click(Locator),
    DblClick(Locator),
    Hover(Locator),
    Fill(Locator, String),
    Check(Locator),
    Uncheck(Locator),
    Select(Locator, String),
    Press(Option<Locator>, String),
    Upload(Locator, String),
    Wait(WaitSpec),
    Expect(BrowserExpect),
    Capture(Vec<(String, BrowserCapture)>),
    Screenshot,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WaitSpec {
    Time(Duration),
    Url(String),
    Visible(Locator),
    Hidden(Locator),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BrowserCapture {
    FromUrl(String),
    Text(Locator),
    Value(Locator),
    Attribute(Locator, String),
}

/// Find an element the way people describe it.
#[derive(Debug, Clone, PartialEq)]
pub struct Locator {
    pub by: By,
    pub within: Option<Box<Locator>>,
    pub nth: Option<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum By {
    Role {
        role: String,
        name: Option<String>,
        exact: bool,
    },
    Label {
        text: String,
        exact: bool,
    },
    Text {
        text: String,
        exact: bool,
    },
    Placeholder(String),
    TestId(String),
    Css(String),
    /// A button or link with this name (string shorthand for `click`).
    Clickable(String),
    /// The whole page (or the `within` scope).
    Page,
}

pub const ARIA_ROLES: &[&str] = &[
    "alert",
    "alertdialog",
    "application",
    "article",
    "banner",
    "blockquote",
    "button",
    "caption",
    "cell",
    "checkbox",
    "code",
    "columnheader",
    "combobox",
    "complementary",
    "contentinfo",
    "definition",
    "deletion",
    "dialog",
    "directory",
    "document",
    "emphasis",
    "feed",
    "figure",
    "form",
    "generic",
    "grid",
    "gridcell",
    "group",
    "heading",
    "img",
    "insertion",
    "link",
    "list",
    "listbox",
    "listitem",
    "log",
    "main",
    "marquee",
    "math",
    "meter",
    "menu",
    "menubar",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "navigation",
    "none",
    "note",
    "option",
    "paragraph",
    "presentation",
    "progressbar",
    "radio",
    "radiogroup",
    "region",
    "row",
    "rowgroup",
    "rowheader",
    "scrollbar",
    "search",
    "searchbox",
    "separator",
    "slider",
    "spinbutton",
    "status",
    "strong",
    "subscript",
    "superscript",
    "switch",
    "tab",
    "table",
    "tablist",
    "tabpanel",
    "term",
    "textbox",
    "time",
    "timer",
    "toolbar",
    "tooltip",
    "tree",
    "treegrid",
    "treeitem",
];

const LOCATOR_KEYS: &[&str] = &["role", "name", "exact", "label", "text", "placeholder", "test_id", "testid", "css", "within", "nth"];

impl Locator {
    pub fn page() -> Self {
        Locator { by: By::Page, within: None, nth: None }
    }

    /// From a value; strings mean `text` unless `clickable` is set.
    pub fn from_value(v: &Value, clickable: bool) -> Result<Self, String> {
        match v {
            Value::String(s) => Ok(Locator { by: if clickable { By::Clickable(s.clone()) } else { By::Text { text: s.clone(), exact: false } }, within: None, nth: None }),
            Value::Object(m) => Self::from_map(m),
            _ => Err(format!("expected a locator like {{ role: button, name: Save }}, got {v}")),
        }
    }

    /// From the locator keys of a map (other keys are ignored by the caller).
    pub fn from_map(m: &Map<String, Value>) -> Result<Self, String> {
        let exact = m.get("exact").and_then(Value::as_bool).unwrap_or(false);
        let name = m.get("name").map(|n| as_str(n, "name")).transpose()?;
        let mut by = None;
        if let Some(r) = m.get("role") {
            let role = as_str(r, "role")?.to_ascii_lowercase();
            if !ARIA_ROLES.contains(&role.as_str()) {
                return Err(format!("unknown ARIA role `{role}`"));
            }
            by = Some(By::Role { role, name: name.clone(), exact });
        }
        for (k, v) in m {
            if ARIA_ROLES.contains(&k.as_str()) && !matches!(k.as_str(), "text" | "code") {
                if by.is_some() {
                    return Err(format!("locator has both a role and `{k}`"));
                }
                by = Some(By::Role { role: k.clone(), name: Some(as_str(v, k)?), exact });
            }
        }
        let simple = [("label", 0), ("text", 1), ("placeholder", 2), ("test_id", 3), ("testid", 3), ("css", 4)];
        for (key, kind) in simple {
            if let Some(v) = m.get(key) {
                if by.is_some() {
                    return Err(format!("locator mixes `{key}` with another way of finding the element"));
                }
                let s = as_str(v, key)?;
                by = Some(match kind {
                    0 => By::Label { text: s, exact },
                    1 => By::Text { text: s, exact },
                    2 => By::Placeholder(s),
                    3 => By::TestId(s),
                    _ => By::Css(s),
                });
            }
        }
        let within = match m.get("within") {
            Some(w) => Some(Box::new(Locator::from_value(w, false)?)),
            None => None,
        };
        let by = match by {
            Some(b) => b,
            None if name.is_some() => return Err("`name` needs a `role` (e.g. role: button)".into()),
            None if within.is_some() => By::Page,
            None => return Err("locator needs one of role, label, text, placeholder, test_id or css".into()),
        };
        let nth = m.get("nth").and_then(Value::as_i64).map(|n| n as i32);
        Ok(Locator { by, within, nth })
    }

    pub fn describe(&self) -> String {
        let base = match &self.by {
            By::Role { role, name: Some(n), .. } => format!("{role} \"{n}\""),
            By::Role { role, name: None, .. } => role.clone(),
            By::Label { text, .. } => format!("field labelled \"{text}\""),
            By::Text { text, .. } => format!("text \"{text}\""),
            By::Placeholder(p) => format!("field with placeholder \"{p}\""),
            By::TestId(t) => format!("test id \"{t}\""),
            By::Css(c) => format!("css `{c}`"),
            By::Clickable(n) => format!("\"{n}\""),
            By::Page => "the page".into(),
        };
        let base = match self.nth {
            Some(n) => format!("{base} #{}", n + 1),
            None => base,
        };
        match &self.within {
            Some(w) => format!("{base} within {}", w.describe()),
            None => base,
        }
    }
}

fn has_locator_keys(m: &Map<String, Value>) -> bool {
    m.keys().any(|k| (LOCATOR_KEYS.contains(&k.as_str()) && !matches!(k.as_str(), "text" | "exact" | "nth" | "within")) || (ARIA_ROLES.contains(&k.as_str()) && k != "text"))
}

fn locator_part(m: &Map<String, Value>, drop: &[&str]) -> Map<String, Value> {
    m.iter().filter(|(k, _)| !drop.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct BrowserExpect {
    pub target: Locator,
    pub checks: Vec<UiCheck>,
    pub timeout: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiCheck {
    Visible(bool),
    /// Element text contains / equals.
    Text {
        text: String,
        exact: bool,
    },
    Value(String),
    Checked(bool),
    Enabled(bool),
    Count(usize),
    Url(String),
    Title(String),
}

impl BrowserExpect {
    pub fn from_value(v: &Value) -> Result<Self, String> {
        let m = match v {
            Value::String(s) => {
                let mut m = Map::new();
                m.insert("text".into(), Value::String(s.clone()));
                m
            }
            Value::Object(m) => m.clone(),
            _ => return Err(format!("expect needs a mapping, e.g. {{ text: \"Saved\" }}, got {v}")),
        };
        let allowed_extra = ["visible", "hidden", "text", "exact_text", "value", "checked", "enabled", "disabled", "count", "url", "title", "timeout"];
        for k in m.keys() {
            if !LOCATOR_KEYS.contains(&k.as_str()) && !ARIA_ROLES.contains(&k.as_str()) && !allowed_extra.contains(&k.as_str()) {
                return Err(format!("unknown key `{k}` in expect"));
            }
        }
        let mut checks = Vec::new();
        let target = if has_locator_keys(&m) {
            // A located element, optionally with text etc. to check on it.
            let loc = locator_part(&m, &allowed_extra);
            let loc = Locator::from_map(&loc)?;
            if let Some(t) = m.get("text") {
                checks.push(UiCheck::Text { text: as_str(t, "text")?, exact: false });
            }
            loc
        } else if let Some(t) = m.get("text") {
            // `text:` alone (optionally `within:`): that text is visible.
            let within = match m.get("within") {
                Some(w) => Some(Box::new(Locator::from_value(w, false)?)),
                None => None,
            };
            let exact = m.get("exact").and_then(Value::as_bool).unwrap_or(false);
            Locator { by: By::Text { text: as_str(t, "text")?, exact }, within, nth: None }
        } else {
            Locator::page()
        };
        if let Some(t) = m.get("exact_text") {
            checks.push(UiCheck::Text { text: as_str(t, "exact_text")?, exact: true });
        }
        if let Some(v) = m.get("value") {
            checks.push(UiCheck::Value(as_str(v, "value")?));
        }
        if let Some(v) = m.get("checked") {
            checks.push(UiCheck::Checked(v.as_bool().ok_or("`checked` must be true or false")?));
        }
        if let Some(v) = m.get("enabled") {
            checks.push(UiCheck::Enabled(v.as_bool().ok_or("`enabled` must be true or false")?));
        }
        if let Some(v) = m.get("disabled") {
            checks.push(UiCheck::Enabled(!v.as_bool().ok_or("`disabled` must be true or false")?));
        }
        if let Some(v) = m.get("count") {
            checks.push(UiCheck::Count(v.as_u64().ok_or("`count` must be a whole number")? as usize));
        }
        if let Some(v) = m.get("url") {
            checks.push(UiCheck::Url(as_str(v, "url")?));
        }
        if let Some(v) = m.get("title") {
            checks.push(UiCheck::Title(as_str(v, "title")?));
        }
        if let Some(v) = m.get("hidden") {
            checks.push(UiCheck::Visible(!v.as_bool().unwrap_or(true)));
        } else if let Some(v) = m.get("visible") {
            checks.push(UiCheck::Visible(v.as_bool().unwrap_or(true)));
        } else if target.by != By::Page && !checks.iter().any(|c| matches!(c, UiCheck::Count(_))) {
            checks.insert(0, UiCheck::Visible(true));
        }
        if checks.is_empty() {
            return Err("expect checks nothing; add text, url, title, visible, checked, count…".into());
        }
        let timeout = m.get("timeout").map(parse_duration).transpose()?;
        Ok(BrowserExpect { target, checks, timeout })
    }

    pub fn describe(&self) -> String {
        let parts: Vec<String> = self
            .checks
            .iter()
            .map(|c| match c {
                UiCheck::Visible(true) => "is visible".into(),
                UiCheck::Visible(false) => "is not visible".into(),
                UiCheck::Text { text, exact: false } => format!("contains \"{text}\""),
                UiCheck::Text { text, exact: true } => format!("reads exactly \"{text}\""),
                UiCheck::Value(v) => format!("has value \"{v}\""),
                UiCheck::Checked(true) => "is checked".into(),
                UiCheck::Checked(false) => "is not checked".into(),
                UiCheck::Enabled(true) => "is enabled".into(),
                UiCheck::Enabled(false) => "is disabled".into(),
                UiCheck::Count(n) => format!("appears {n} times"),
                UiCheck::Url(u) => format!("URL matches \"{u}\""),
                UiCheck::Title(t) => format!("title contains \"{t}\""),
            })
            .collect();
        format!("{} {}", self.target.describe(), parts.join(", "))
    }
}

const ACTIONS: &[&str] = &["goto", "click", "dblclick", "hover", "fill", "check", "uncheck", "select", "press", "upload", "wait", "expect", "capture", "screenshot"];

impl BrowserSpec {
    /// Accepts a list of actions, `{ as, steps: [...] }`, or a single action map.
    pub fn from_value(v: &Value) -> Result<Self, String> {
        let (as_profile, list): (Option<String>, Vec<Value>) = match v {
            Value::Array(a) => (None, a.clone()),
            Value::Object(m) if m.contains_key("steps") => {
                check_keys(m, &["as", "steps"], "a browser block")?;
                let steps = m["steps"].as_array().cloned().ok_or("`steps` must be a list of actions")?;
                (m.get("as").map(|a| as_str(a, "as")).transpose()?, steps)
            }
            Value::Object(m) => {
                // Single action map, possibly with `as`.
                let as_p = m.get("as").map(|a| as_str(a, "as")).transpose()?;
                let rest: Map<String, Value> = m.iter().filter(|(k, _)| k.as_str() != "as").map(|(k, v)| (k.clone(), v.clone())).collect();
                let list = rest.into_iter().map(|(k, v)| Value::Object([(k, v)].into_iter().collect())).collect();
                (as_p, list)
            }
            _ => return Err("a browser block is a list of actions, e.g. `- click: { role: button, name: Save }`".into()),
        };
        let actions = list.iter().map(Action::from_value).collect::<Result<Vec<_>, _>>()?;
        if actions.is_empty() {
            return Err("browser block has no actions".into());
        }
        Ok(BrowserSpec { as_profile, actions })
    }
}

impl Action {
    pub fn from_value(v: &Value) -> Result<Self, String> {
        if v.as_str() == Some("screenshot") {
            return Ok(Action::Screenshot);
        }
        let m = obj(v, "an action")?;
        if m.len() != 1 {
            return Err(format!("each action is one key ({}); got {}", ACTIONS.join(", "), m.keys().cloned().collect::<Vec<_>>().join(", ")));
        }
        let (k, val) = m.iter().next().unwrap();
        let with_field = |field: &str| -> Result<(Locator, String), String> {
            let mm = obj(val, k)?;
            let f = mm.get(field).ok_or_else(|| format!("`{k}` needs `{field}`"))?;
            let loc = Locator::from_map(&locator_part(mm, &[field]))?;
            Ok((loc, as_str(f, field)?))
        };
        Ok(match k.as_str() {
            "goto" => Action::Goto(as_str(val, "goto")?),
            "click" => Action::Click(Locator::from_value(val, true)?),
            "dblclick" => Action::DblClick(Locator::from_value(val, true)?),
            "hover" => Action::Hover(Locator::from_value(val, true)?),
            "check" => Action::Check(Locator::from_value(val, false)?),
            "uncheck" => Action::Uncheck(Locator::from_value(val, false)?),
            "fill" => {
                let (l, v) = with_field("value")?;
                Action::Fill(l, v)
            }
            "select" => {
                let mm = obj(val, k)?;
                let field = if mm.contains_key("option") { "option" } else { "value" };
                let (l, v) = with_field(field)?;
                Action::Select(l, v)
            }
            "upload" => {
                let (l, v) = with_field("file")?;
                Action::Upload(l, v)
            }
            "press" => match val {
                Value::String(key) => Action::Press(None, key.clone()),
                Value::Object(mm) => {
                    let key = as_str(mm.get("key").ok_or("`press` needs `key`")?, "key")?;
                    let rest = locator_part(mm, &["key"]);
                    let loc = if rest.is_empty() { None } else { Some(Locator::from_map(&rest)?) };
                    Action::Press(loc, key)
                }
                _ => return Err("`press` needs a key name like Enter".into()),
            },
            "wait" => Action::Wait(match val {
                Value::Number(_) | Value::String(_) => WaitSpec::Time(parse_duration(val)?),
                Value::Object(mm) if mm.contains_key("url") => WaitSpec::Url(as_str(&mm["url"], "url")?),
                Value::Object(mm) if mm.get("hidden").and_then(Value::as_bool) == Some(true) => WaitSpec::Hidden(Locator::from_map(&locator_part(mm, &["hidden"]))?),
                Value::Object(mm) => WaitSpec::Visible(Locator::from_map(&locator_part(mm, &["visible"]))?),
                _ => return Err("`wait` needs a duration, {url: …} or a locator".into()),
            }),
            "expect" => Action::Expect(BrowserExpect::from_value(val)?),
            "capture" => Action::Capture(obj(val, "capture")?.iter().map(|(name, spec)| Ok((name.clone(), BrowserCapture::from_value(spec)?))).collect::<Result<_, String>>()?),
            "screenshot" => Action::Screenshot,
            other => return Err(format!("unknown action `{other}` (known: {})", ACTIONS.join(", "))),
        })
    }

    pub fn describe(&self) -> String {
        match self {
            Action::Goto(u) => format!("open {u}"),
            Action::Click(l) => format!("click {}", l.describe()),
            Action::DblClick(l) => format!("double-click {}", l.describe()),
            Action::Hover(l) => format!("hover {}", l.describe()),
            Action::Fill(l, _) => format!("fill {}", l.describe()),
            Action::Check(l) => format!("tick {}", l.describe()),
            Action::Uncheck(l) => format!("untick {}", l.describe()),
            Action::Select(l, v) => format!("choose \"{v}\" in {}", l.describe()),
            Action::Press(Some(l), k) => format!("press {k} in {}", l.describe()),
            Action::Press(None, k) => format!("press {k}"),
            Action::Upload(l, f) => format!("upload {f} to {}", l.describe()),
            Action::Wait(WaitSpec::Time(d)) => format!("wait {}ms", d.as_millis()),
            Action::Wait(WaitSpec::Url(u)) => format!("wait for URL {u}"),
            Action::Wait(WaitSpec::Visible(l)) => format!("wait for {}", l.describe()),
            Action::Wait(WaitSpec::Hidden(l)) => format!("wait for {} to go", l.describe()),
            Action::Expect(e) => format!("check {}", e.describe()),
            Action::Capture(c) => format!("keep {}", c.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")),
            Action::Screenshot => "screenshot".into(),
        }
    }
}

impl BrowserCapture {
    fn from_value(v: &Value) -> Result<Self, String> {
        let m = obj(v, "a browser capture")?;
        if let Some(r) = m.get("from_url") {
            let r = as_str(r, "from_url")?;
            regex::Regex::new(&r).map_err(|e| format!("bad regex: {e}"))?;
            return Ok(BrowserCapture::FromUrl(r));
        }
        if let Some(l) = m.get("text") {
            return Ok(BrowserCapture::Text(Locator::from_value(l, false)?));
        }
        if let Some(l) = m.get("value") {
            return Ok(BrowserCapture::Value(Locator::from_value(l, false)?));
        }
        if let (Some(a), Some(of)) = (m.get("attribute"), m.get("of")) {
            return Ok(BrowserCapture::Attribute(Locator::from_value(of, false)?, as_str(a, "attribute")?));
        }
        Err("browser capture needs {from_url: regex}, {text: locator}, {value: locator} or {attribute: name, of: locator}".into())
    }
}

// ---------------------------------------------------------------- use

#[derive(Debug, Clone, PartialEq)]
pub struct UseSpec {
    /// Fragment path relative to `fragments/`, without `.flow.md`.
    pub fragment: String,
    pub params: Map<String, Value>,
}

impl UseSpec {
    pub fn from_value(v: &Value) -> Result<Self, String> {
        match v {
            Value::String(s) => Ok(UseSpec { fragment: s.clone(), params: Map::new() }),
            Value::Object(m) if m.len() == 1 => {
                let (k, p) = m.iter().next().unwrap();
                let params = match p {
                    Value::Object(pm) => pm.clone(),
                    Value::Null => Map::new(),
                    _ => return Err("fragment parameters must be a mapping".into()),
                };
                Ok(UseSpec { fragment: k.clone(), params })
            }
            _ => Err("use needs a fragment name, e.g. `team/invite-case-handler: { branch: Bristol }`".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_yaml;

    fn y(s: &str) -> Value {
        parse_yaml(s).unwrap()
    }

    #[test]
    fn http_block() {
        let h = HttpSpec::from_value(&y("POST: \"{{env.API}}/quotes\"\nauth: admin\njson: { a: 1 }\nexpect: { status: 201, json: { $.total: 136800, status: { one_of: [a, b] } } }\ncapture: { id: $.id, loc: { header: Location } }\neventually: 10s")).unwrap();
        assert_eq!(h.method, "POST");
        assert_eq!(h.expect.status, vec![201]);
        assert_eq!(h.expect.json[1].0, "$.status");
        assert_eq!(h.eventually, Some(Duration::from_secs(10)));
        assert!(HttpSpec::from_value(&y("GET: a\nPOST: b")).is_err());
        assert!(HttpSpec::from_value(&y("GET: a\nbogus: 1")).is_err());
    }

    #[test]
    fn browser_shapes() {
        let a = BrowserSpec::from_value(&y("- goto: /x\n- click: { role: button, name: Save }\n- click: Send quote\n- fill: { label: Email, value: a@b }")).unwrap();
        assert_eq!(a.actions.len(), 4);
        assert!(matches!(&a.actions[2], Action::Click(Locator { by: By::Clickable(n), .. }) if n == "Send quote"));
        let b = BrowserSpec::from_value(&y("as: case-handler\nsteps:\n  - goto: /lead")).unwrap();
        assert_eq!(b.as_profile.as_deref(), Some("case-handler"));
        let c = BrowserSpec::from_value(&y("goto: /x")).unwrap();
        assert_eq!(c.actions, vec![Action::Goto("/x".into())]);
    }

    #[test]
    fn expect_shapes() {
        let e = BrowserExpect::from_value(&y("{ text: \"£36.00\", within: { role: region, name: Order summary } }")).unwrap();
        assert!(matches!(e.target.by, By::Text { .. }));
        assert!(e.target.within.is_some());
        let e = BrowserExpect::from_value(&y("{ heading: Your quote }")).unwrap();
        assert!(matches!(&e.target.by, By::Role { role, .. } if role == "heading"));
        assert_eq!(e.checks, vec![UiCheck::Visible(true)]);
        let e = BrowserExpect::from_value(&y("{ role: checkbox, name: Quote accepted, checked: true }")).unwrap();
        assert_eq!(e.checks, vec![UiCheck::Visible(true), UiCheck::Checked(true)]);
        let e = BrowserExpect::from_value(&y("{ url: /paid }")).unwrap();
        assert_eq!(e.target.by, By::Page);
        assert!(BrowserExpect::from_value(&y("{ role: buttn }")).is_err());
    }

    #[test]
    fn matchers() {
        assert!(Matcher::from_value(&y("200")).unwrap().check(Some(&Value::from("200"))).is_ok());
        assert!(Matcher::from_value(&y("{ gt: 3 }")).unwrap().check(Some(&Value::from(4))).is_ok());
        assert!(Matcher::from_value(&y("{ contains: ab }")).unwrap().check(Some(&Value::from("xaby"))).is_ok());
        assert!(Matcher::from_value(&y("paid")).unwrap().check(Some(&Value::from("pending"))).is_err());
        assert!(Matcher::from_value(&y("{ exists: false }")).unwrap().check(None).is_ok());
    }
}

// ---------------------------------------------------------------- exec

/// How to run a command: through the shell, or as an argument list.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Shell(String),
    Args(Vec<String>),
}

impl Command {
    pub fn display(&self) -> String {
        match self {
            Command::Shell(s) => s.clone(),
            Command::Args(a) => a.iter().map(|x| if x.contains(' ') { format!("{x:?}") } else { x.clone() }).collect::<Vec<_>>().join(" "),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecSpec {
    pub run: Command,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub stdin: Option<String>,
    pub timeout: Option<Duration>,
    pub expect: ExecExpect,
    pub capture: Vec<(String, ExecCapture)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecExpect {
    /// Allowed exit codes (default: 0). Empty = any.
    pub exit: Option<Vec<i32>>,
    pub stdout: Option<Matcher>,
    pub stderr: Option<Matcher>,
    /// JSONPath checks on stdout parsed as JSON.
    pub json: Vec<(String, Matcher)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExecCapture {
    /// First group (or the whole match) of a regex over stdout.
    Regex(String),
    /// A JSONPath into stdout parsed as JSON.
    Json(String),
    Exit,
}

fn text_matcher(v: &Value) -> Result<Matcher, String> {
    match v {
        Value::String(s) => Ok(Matcher::Contains(Value::String(s.clone()))),
        other => Matcher::from_value(other),
    }
}

impl ExecSpec {
    pub fn from_value(v: &Value) -> Result<Self, String> {
        let m = match v {
            Value::String(s) => {
                return Ok(ExecSpec {
                    run: Command::Shell(s.clone()),
                    cwd: None,
                    env: vec![],
                    stdin: None,
                    timeout: None,
                    expect: ExecExpect { exit: Some(vec![0]), stdout: None, stderr: None, json: vec![] },
                    capture: vec![],
                });
            }
            _ => obj(v, "an exec block")?,
        };
        check_keys(m, &["run", "cwd", "env", "stdin", "timeout", "expect", "capture"], "an exec block")?;
        let run = match m.get("run") {
            Some(Value::String(s)) => Command::Shell(s.clone()),
            Some(Value::Array(a)) => Command::Args(
                a.iter()
                    .map(|x| match x {
                        Value::String(s) => s.clone(),
                        other => crate::results::value_preview(other),
                    })
                    .collect(),
            ),
            _ => return Err("needs `run`: a shell command, or a list of arguments".into()),
        };
        let mut expect = ExecExpect { exit: Some(vec![0]), stdout: None, stderr: None, json: vec![] };
        if let Some(e) = m.get("expect") {
            let em = obj(e, "exec expect")?;
            check_keys(em, &["exit", "stdout", "stderr", "json"], "exec expect")?;
            expect.exit = match em.get("exit") {
                None => Some(vec![0]),
                Some(x) => exit_codes(x)?,
            };
            expect.stdout = em.get("stdout").map(text_matcher).transpose()?;
            expect.stderr = em.get("stderr").map(text_matcher).transpose()?;
            if let Some(j) = em.get("json") {
                for (k, v) in obj(j, "expect.json")? {
                    let path = if k.starts_with('$') { k.clone() } else { format!("$.{k}") };
                    serde_json_path::JsonPath::parse(&path).map_err(|e| format!("bad JSONPath `{k}`: {e}"))?;
                    expect.json.push((path, Matcher::from_value(v)?));
                }
            }
        }
        let capture = match m.get("capture") {
            None => vec![],
            Some(c) => obj(c, "capture")?
                .iter()
                .map(|(k, v)| {
                    let c = match v {
                        Value::String(s) if s == "exit" => ExecCapture::Exit,
                        Value::String(s) if s.starts_with('$') => ExecCapture::Json(s.clone()),
                        Value::Object(cm) if cm.contains_key("json") => {
                            let p = as_str(&cm["json"], "json")?;
                            ExecCapture::Json(if p.starts_with('$') { p } else { format!("$.{p}") })
                        }
                        Value::Object(cm) if cm.contains_key("regex") || cm.contains_key("stdout") => {
                            let r = as_str(cm.get("regex").or(cm.get("stdout")).unwrap(), "regex")?;
                            regex::Regex::new(&r).map_err(|e| format!("bad regex: {e}"))?;
                            ExecCapture::Regex(r)
                        }
                        _ => return Err(format!("capture `{k}` needs a JSONPath like `$.id`, {{regex: …}} or `exit`")),
                    };
                    Ok((k.clone(), c))
                })
                .collect::<Result<_, String>>()?,
        };
        Ok(ExecSpec {
            run,
            cwd: m.get("cwd").map(|c| as_str(c, "cwd")).transpose()?,
            env: str_pairs(m.get("env").unwrap_or(&Value::Null), "env")?,
            stdin: m.get("stdin").map(|c| as_str(c, "stdin")).transpose()?,
            timeout: m.get("timeout").map(parse_duration).transpose()?,
            expect,
            capture,
        })
    }
}

// ---------------------------------------------------------------- email

/// ```email
/// to: "jane+{{run.short_id}}@example.test"
/// subject: Your quote          # optional: pick the latest message whose subject contains this
/// within: 30s
/// expect: { body: "£1,450.00" }
/// capture: { accept_link: { link: "/accept/" } }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct EmailSpec {
    pub to: String,
    pub subject: Option<String>,
    pub within: Option<Duration>,
    pub expect: EmailExpect,
    pub capture: Vec<(String, EmailCapture)>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EmailExpect {
    pub subject: Option<Matcher>,
    /// Matched against the text body (or the HTML with tags removed).
    pub body: Option<Matcher>,
    pub from: Option<Matcher>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EmailCapture {
    /// The first link in the message whose address contains this text ("" = the first link).
    Link(String),
    /// First group (or the whole match) of a regex over the text body.
    Regex(String),
    Subject,
}

impl EmailSpec {
    pub fn from_value(v: &Value) -> Result<Self, String> {
        let m = obj(v, "an email block")?;
        check_keys(m, &["to", "subject", "within", "expect", "capture"], "an email block")?;
        let to = m.get("to").map(|t| as_str(t, "to")).transpose()?.ok_or("an email block needs `to`: the address the message was sent to")?;
        let mut expect = EmailExpect::default();
        if let Some(e) = m.get("expect") {
            let em = obj(e, "email expect")?;
            check_keys(em, &["subject", "body", "from"], "email expect")?;
            expect.subject = em.get("subject").map(text_matcher).transpose()?;
            expect.body = em.get("body").map(text_matcher).transpose()?;
            expect.from = em.get("from").map(text_matcher).transpose()?;
        }
        let capture = match m.get("capture") {
            None => vec![],
            Some(c) => obj(c, "capture")?
                .iter()
                .map(|(k, v)| {
                    let c = match v {
                        Value::String(s) if s == "subject" => EmailCapture::Subject,
                        Value::String(s) if s == "link" => EmailCapture::Link(String::new()),
                        Value::Object(cm) if cm.contains_key("link") => EmailCapture::Link(as_str(&cm["link"], "link")?),
                        Value::Object(cm) if cm.contains_key("regex") => {
                            let r = as_str(&cm["regex"], "regex")?;
                            regex::Regex::new(&r).map_err(|e| format!("bad regex: {e}"))?;
                            EmailCapture::Regex(r)
                        }
                        _ => return Err(format!("capture `{k}` needs {{ link: \"/accept/\" }}, {{ regex: … }} or `subject`")),
                    };
                    Ok((k.clone(), c))
                })
                .collect::<Result<_, String>>()?,
        };
        Ok(EmailSpec { to, subject: m.get("subject").map(|s| as_str(s, "subject")).transpose()?, within: m.get("within").map(parse_duration).transpose()?, expect, capture })
    }
}
