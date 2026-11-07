//! A clickable prototype built from a flow's design blocks: each screen is
//! served at the `path` it declares, and storyboard transitions become
//! navigation (clicking “Accept quote” on `quote` opens `pay`). Browser steps
//! can run against it unchanged before the feature exists.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use enocone_engine::Project;
use enocone_spec::Flow;
use enocone_spec::design::Transition;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone)]
pub struct ProtoScreen {
    pub name: String,
    pub title: Option<String>,
    /// e.g. `/public/quotes/{quote_id}/accept`; screens without one are served at `/__screens/<name>`.
    pub path: Option<String>,
    pub html: String,
}

#[derive(Debug, Clone, Default)]
pub struct Prototype {
    pub screens: Vec<ProtoScreen>,
    pub transitions: Vec<Transition>,
    pub sample: Map<String, Value>,
    pub theme: Option<String>,
}

impl Prototype {
    /// Everything the given flows design: their screens, storyboards and sample values.
    pub fn from_flows(project: &Project, flows: &[Flow]) -> Prototype {
        let mut p = Prototype { theme: project.theme_css(), ..Default::default() };
        for f in flows {
            let all = f.designs.iter().chain(f.steps().flat_map(|s| s.designs.iter()));
            for d in all {
                for sc in d.screen.iter().chain(d.screens.iter()) {
                    let (Some(name), Some(html)) = (&sc.name, &sc.html) else { continue };
                    match p.screens.iter_mut().find(|x| &x.name == name) {
                        Some(existing) => {
                            // A later block may add the path or title.
                            if existing.path.is_none() {
                                existing.path = sc.path.clone();
                            }
                            if existing.title.is_none() {
                                existing.title = sc.title.clone();
                            }
                        }
                        None => p.screens.push(ProtoScreen { name: name.clone(), title: sc.title.clone(), path: sc.path.clone(), html: html.clone() }),
                    }
                }
                for t in &d.transitions {
                    if !p.transitions.contains(t) {
                        p.transitions.push(t.clone());
                    }
                }
            }
            for (k, v) in &f.sample {
                p.sample.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        p
    }

    fn find(&self, url_path: &str) -> Option<(&ProtoScreen, BTreeMap<String, String>)> {
        for sc in &self.screens {
            if let Some(pattern) = &sc.path
                && let Some(params) = match_path(pattern, url_path)
            {
                return Some((sc, params));
            }
            if url_path == format!("/__screens/{}", sc.name) {
                return Some((sc, BTreeMap::new()));
            }
        }
        None
    }

    /// The URL of a screen, filling `{param}`s from the current page, then sample values.
    pub fn url_of(&self, name: &str, params: &BTreeMap<String, String>) -> String {
        let Some(sc) = self.screens.iter().find(|s| s.name == name) else { return format!("/__screens/{name}") };
        let Some(pattern) = &sc.path else { return format!("/__screens/{name}") };
        pattern
            .split('/')
            .map(|seg| match param_name(seg) {
                Some(p) => params.get(p).cloned().or_else(|| self.sample.get(p).map(value_text)).unwrap_or_else(|| "sample".into()),
                None => seg.to_string(),
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    fn page(&self, sc: &ProtoScreen, params: &BTreeMap<String, String>) -> String {
        let links: Vec<Value> = self.transitions.iter().filter(|t| t.from == sc.name).map(|t| json!({ "text": trigger_text(t.on.as_ref()), "to": self.url_of(&t.to, params) })).collect();
        let script = format!(
            r#"<script>(() => {{
  const links = {links};
  const norm = (s) => (s || "").replace(/\s+/g, " ").trim().toLowerCase();
  const nameOf = (el) => norm(el.getAttribute("aria-label") || el.innerText || el.value || el.textContent);
  const go = (el) => {{
    const n = nameOf(el);
    const hit = links.find((l) => l.text === "" || (n && (n === norm(l.text) || n.includes(norm(l.text)))));
    if (hit) {{ location.href = hit.to; return true; }}
    return false;
  }};
  document.addEventListener("click", (e) => {{
    const el = e.target.closest("button, a, [role=button], input[type=submit], input[type=button]");
    if (!el) return;
    if (go(el) || el.tagName === "A" || el.type === "submit" || el.tagName === "BUTTON") e.preventDefault();
  }}, true);
  // A mockup's forms have nowhere to post to.
  document.addEventListener("submit", (e) => e.preventDefault(), true);
}})();</script>"#,
            links = Value::Array(links)
        );
        let mut doc = crate::app::screen_document(&sc.html, self.theme.as_deref());
        let title = format!("<title>{}</title>", escape(sc.title.as_deref().unwrap_or(&sc.name)));
        doc = doc.replacen("<meta charset=\"utf-8\">", &format!("<meta charset=\"utf-8\">{title}"), 1);
        match doc.rfind("</body>") {
            Some(i) => format!("{}{script}{}", &doc[..i], &doc[i..]),
            None => format!("{doc}{script}"),
        }
    }

    fn index(&self) -> String {
        let items: String =
            self.screens.iter().map(|s| format!("<li><a href=\"{}\">{}</a></li>", escape(&self.url_of(&s.name, &BTreeMap::new())), escape(s.title.as_deref().unwrap_or(&s.name)))).collect();
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>Prototype</title><style>body{{font-family:system-ui,sans-serif;margin:40px;color:#12151C}}a{{color:#3D4FE0}}li{{margin:6px 0}}</style></head><body><h1>Prototype</h1><p>Screens from the flow documents, linked by their storyboards.</p><ul>{items}</ul></body></html>"
        )
    }
}

/// Start the prototype server on `addr` (port 0 picks a free one). It stops when the handle is aborted.
pub async fn serve(proto: Prototype, addr: SocketAddr) -> anyhow::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    let app = Router::new().fallback(handle).with_state(Arc::new(proto));
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((local, handle))
}

async fn handle(State(p): State<Arc<Prototype>>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    if let Some((sc, params)) = p.find(&path) {
        return Html(p.page(sc, &params)).into_response();
    }
    if path == "/" {
        return Html(p.index()).into_response();
    }
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Not in the prototype</title></head><body style=\"font-family:system-ui,sans-serif;margin:40px\"><h1>Not in the prototype</h1><p>No screen declares the path <code>{}</code>. Give a screen block <code>path: …</code> or see <a href=\"/\">all screens</a>.</p></body></html>",
        escape(&path)
    );
    (StatusCode::NOT_FOUND, Html(body)).into_response()
}

fn param_name(seg: &str) -> Option<&str> {
    seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')).or_else(|| seg.strip_prefix(':')).filter(|s| !s.is_empty())
}

fn match_path(pattern: &str, path: &str) -> Option<BTreeMap<String, String>> {
    let a: Vec<&str> = pattern.trim_end_matches('/').split('/').collect();
    let b: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    if a.len() != b.len() {
        return None;
    }
    let mut params = BTreeMap::new();
    for (p, s) in a.iter().zip(&b) {
        match param_name(p) {
            Some(name) if !s.is_empty() => {
                params.insert(name.to_string(), s.to_string());
            }
            _ if p == s => {}
            _ => return None,
        }
    }
    Some(params)
}

/// What the user clicks: `{ click: Accept quote }`, `{ click: { name: … } }`, or a bare string.
fn trigger_text(on: Option<&Value>) -> String {
    let Some(on) = on else { return String::new() };
    let pick = |v: &Value| -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Object(m) => ["name", "text", "label"].iter().find_map(|k| m.get(*k).and_then(Value::as_str)).unwrap_or("").to_string(),
            _ => String::new(),
        }
    };
    match on {
        Value::Object(m) => m.get("click").or_else(|| m.values().next()).map(pick).unwrap_or_default(),
        other => pick(other),
    }
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_paths_and_fills_params() {
        let m = match_path("/public/quotes/{quote_id}/accept", "/public/quotes/q-9/accept").unwrap();
        assert_eq!(m["quote_id"], "q-9");
        assert!(match_path("/public/quotes/{quote_id}", "/public/quotes/q-9/accept").is_none());
        let mut p = Prototype::default();
        p.screens.push(ProtoScreen { name: "pay".into(), title: None, path: Some("/q/{quote_id}/pay".into()), html: String::new() });
        p.sample.insert("quote_id".into(), json!("q-sample"));
        assert_eq!(p.url_of("pay", &m), "/q/q-9/pay");
        assert_eq!(p.url_of("pay", &BTreeMap::new()), "/q/q-sample/pay");
        assert_eq!(p.url_of("nope", &BTreeMap::new()), "/__screens/nope");
        assert_eq!(trigger_text(Some(&json!({ "click": "Accept quote" }))), "Accept quote");
        assert_eq!(trigger_text(Some(&json!({ "click": { "role": "button", "name": "Pay" } }))), "Pay");
    }
}
