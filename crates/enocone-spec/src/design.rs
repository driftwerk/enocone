//! Design blocks: flow charts, mockup screens and storyboards. They document
//! a flow (and drive the prototype) but never implement a step.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DesignKind {
    Flowchart,
    Screen,
    Storyboard,
}

impl DesignKind {
    pub fn parse(info: &str) -> Option<Self> {
        match info.split_whitespace().next().unwrap_or("") {
            "flowchart" | "mermaid" => Some(Self::Flowchart),
            "screen" | "mockup" => Some(Self::Screen),
            "storyboard" => Some(Self::Storyboard),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flowchart => "flowchart",
            Self::Screen => "screen",
            Self::Storyboard => "storyboard",
        }
    }
}

/// A design block as written, plus its parsed form.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Design {
    pub kind: DesignKind,
    pub source: String,
    pub line: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart: Option<Chart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<Screen>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<Transition>,
    /// For a storyboard: the screens it shows, loaded from the screens folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screens: Vec<Screen>,
}

// ---------------------------------------------------------------- flowchart

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Chart {
    /// LR, RL, TD or BT.
    pub direction: String,
    pub nodes: Vec<ChartNode>,
    pub edges: Vec<ChartEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChartNode {
    pub id: String,
    pub label: String,
    /// box, round, stadium, diamond, circle, database
    pub shape: String,
    /// The step this box stands for (resolved from a numeric id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChartEdge {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub dashed: bool,
    #[serde(default)]
    pub thick: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

const SHAPES: &[(&str, &str, &str)] =
    &[("[[", "]]", "box"), ("([", "])", "stadium"), ("((", "))", "circle"), ("[(", ")]", "database"), ("[", "]", "box"), ("(", ")", "round"), ("{", "}", "diamond"), (">", "]", "box")];
const EDGES: &[(&str, bool, bool)] = &[("-.->", true, false), ("-.-", true, false), ("==>", false, true), ("===", false, true), ("-->", false, false), ("---", false, false)];

/// The step number a node id refers to: `3` or `s3`.
pub fn step_number(id: &str) -> Option<u32> {
    id.strip_prefix('s').unwrap_or(id).parse().ok().filter(|_| !id.is_empty() && id.chars().any(|c| c.is_ascii_digit()) && id.trim_start_matches('s').chars().all(|c| c.is_ascii_digit()))
}

/// Parse a Mermaid flowchart subset. Errors carry the 0-based line offset.
pub fn parse_chart(src: &str) -> Result<Chart, (usize, String)> {
    let mut chart = Chart { direction: "LR".into(), nodes: vec![], edges: vec![] };
    for (i, raw) in src.lines().enumerate() {
        let line = raw.split("%%").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let first = line.split_whitespace().next().unwrap_or("");
        if matches!(first, "flowchart" | "graph" | "direction") {
            if let Some(d) = line.split_whitespace().nth(1) {
                let d = d.to_ascii_uppercase();
                chart.direction = if d == "TB" { "TD".into() } else { d };
            }
            continue;
        }
        if matches!(first, "classDef" | "class" | "style" | "click" | "linkStyle" | "subgraph" | "end") {
            continue;
        }
        for stmt in line.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            parse_statement(stmt, &mut chart).map_err(|e| (i, e))?;
        }
    }
    if chart.nodes.is_empty() {
        return Err((0, "flowchart has no boxes; write e.g. `1[Quote sent] --> 2[Quote accepted]`".into()));
    }
    Ok(chart)
}

fn parse_statement(s: &str, chart: &mut Chart) -> Result<(), String> {
    let mut rest = s;
    // An arrow waiting for the box it points to: (from, dashed, thick, label).
    let mut arrow: Option<(String, bool, bool, Option<String>)> = None;
    loop {
        let (id, after) = parse_node(rest, chart)?;
        if let Some((from, dashed, thick, label)) = arrow.take() {
            chart.edges.push(ChartEdge { from, to: id.clone(), dashed, thick, label });
        }
        rest = after.trim_start();
        if rest.is_empty() {
            return Ok(());
        }
        let (dashed, thick, label, after) = parse_edge(rest).ok_or_else(|| format!("expected an arrow like `-->` after `{id}`, found `{rest}`"))?;
        rest = after.trim_start();
        if rest.is_empty() {
            return Err(format!("arrow after `{id}` points nowhere"));
        }
        arrow = Some((id, dashed, thick, label));
    }
}

fn parse_edge(s: &str) -> Option<(bool, bool, Option<String>, &str)> {
    // `-- text -->` form
    if let Some(r) = s.strip_prefix("-- ")
        && let Some(end) = r.find("-->")
    {
        return Some((false, false, Some(r[..end].trim().to_string()), &r[end + 3..]));
    }
    for (op, dashed, thick) in EDGES {
        if let Some(r) = s.strip_prefix(op) {
            let r = r.trim_start();
            if let Some(lr) = r.strip_prefix('|')
                && let Some(end) = lr.find('|')
            {
                return Some((*dashed, *thick, Some(lr[..end].trim().trim_matches('"').to_string()), &lr[end + 1..]));
            }
            return Some((*dashed, *thick, None, r));
        }
    }
    None
}

fn parse_node<'a>(s: &'a str, chart: &mut Chart) -> Result<(String, &'a str), String> {
    let s = s.trim_start();
    let end = s.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(s.len());
    let id = &s[..end];
    if id.is_empty() {
        return Err(format!("expected a box id at `{s}`"));
    }
    let mut rest = &s[end..];
    let mut label = None;
    let mut shape = "box";
    for (open, close, name) in SHAPES {
        if let Some(r) = rest.strip_prefix(open) {
            let close_at = r.find(close).ok_or_else(|| format!("box `{id}` opens `{open}` but never closes it with `{close}`"))?;
            label = Some(r[..close_at].trim().trim_matches('"').to_string());
            shape = name;
            rest = &r[close_at + close.len()..];
            break;
        }
    }
    match chart.nodes.iter_mut().find(|n| n.id == id) {
        Some(n) => {
            if let Some(l) = label {
                n.label = l;
                n.shape = shape.into();
            }
        }
        None => chart.nodes.push(ChartNode { id: id.to_string(), label: label.unwrap_or_else(|| id.to_string()), shape: shape.into(), step: None, number: step_number(id) }),
    }
    Ok((id.to_string(), rest))
}

// ---------------------------------------------------------------- screens

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Screen {
    /// Name of `screens/<name>.html`, and how storyboards refer to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Path the prototype serves it at, e.g. `/public/quotes/{quote_id}/accept`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The mockup HTML (inline, or loaded from `screens/<name>.html`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
}

pub fn parse_screen(v: &Value) -> Result<Screen, String> {
    match v {
        Value::String(s) => Ok(Screen { name: Some(s.clone()), ..Default::default() }),
        Value::Object(m) => {
            for k in m.keys() {
                if !matches!(k.as_str(), "screen" | "name" | "title" | "path" | "html") {
                    return Err(format!("unknown key `{k}` in a screen block (screen, title, path, html)"));
                }
            }
            let s = |k: &str| m.get(k).and_then(Value::as_str).map(String::from);
            let sc = Screen { name: s("screen").or_else(|| s("name")), title: s("title"), path: s("path"), html: s("html") };
            if sc.name.is_none() && sc.html.is_none() {
                return Err("a screen needs `screen: <name>` (screens/<name>.html) or inline `html: |`".into());
            }
            Ok(sc)
        }
        _ => Err("a screen block is `screen: <name>`, optionally with title, path or inline html".into()),
    }
}

// ---------------------------------------------------------------- storyboard

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transition {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
    /// What moves the user on, e.g. `{ click: Accept quote }` (drives the prototype).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<Value>,
}

pub fn parse_storyboard(v: &Value) -> Result<Vec<Transition>, String> {
    let list = match v {
        Value::Array(a) => a.clone(),
        Value::Object(m) if m.contains_key("transitions") => m["transitions"].as_array().cloned().ok_or("`transitions` must be a list")?,
        _ => return Err("a storyboard is a list of `- { from: quote, to: pay, step: 3, on: { click: Accept quote } }`".into()),
    };
    list.iter()
        .map(|t| {
            let m = t.as_object().ok_or("each storyboard item is `{ from, to, step, on }`")?;
            let get = |k: &str| m.get(k).and_then(Value::as_str).map(String::from).ok_or(format!("storyboard item needs `{k}`"));
            Ok(Transition { from: get("from")?, to: get("to")?, step: m.get("step").and_then(Value::as_u64).map(|n| n as u32), on: m.get("on").cloned() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mermaid_subset() {
        let c = parse_chart("flowchart TD\n  1[Quote sent] --> 2(\"Opens link\") -.->|later| s3{Waived?}\n  %% comment\n  2 ==> note[Just a picture]\n").unwrap();
        assert_eq!(c.direction, "TD");
        assert_eq!(c.nodes.len(), 4);
        assert_eq!(c.nodes[1].label, "Opens link");
        assert_eq!(c.nodes[2].shape, "diamond");
        assert_eq!(c.nodes[2].number, Some(3));
        assert_eq!(c.nodes[3].number, None);
        assert_eq!(c.edges.len(), 3);
        assert!(c.edges[1].dashed);
        assert_eq!(c.edges[1].label.as_deref(), Some("later"));
        assert!(c.edges[2].thick);
    }

    #[test]
    fn reports_bad_lines() {
        let e = parse_chart("1[a] -->\n").unwrap_err();
        assert!(e.1.contains("points nowhere"), "{e:?}");
        let e = parse_chart("ok[a]\nb[unclosed\n").unwrap_err();
        assert_eq!(e.0, 1);
    }
}
