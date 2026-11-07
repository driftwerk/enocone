//! HTTP steps: send the request, check expectations, capture values.

use std::time::{Duration, Instant};

use enocone_spec::blocks::{HttpBody, HttpCapture, HttpSpec, Matcher};
use enocone_spec::results::{Check, HttpRecord, Poll, StepError};
use serde_json::Value;

use crate::vars::Masker;

const BODY_LIMIT: usize = 64 * 1024;

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    text: String,
    json: Option<Value>,
}

pub struct HttpOutcome {
    pub record: HttpRecord,
    pub captured: Vec<(String, Value)>,
    pub error: Option<StepError>,
}

fn truncate(s: &str) -> String {
    if s.len() <= BODY_LIMIT {
        return s.to_string();
    }
    let mut end = BODY_LIMIT;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… ({} bytes total)", &s[..end], s.len())
}

fn pretty(text: &str) -> String {
    serde_json::from_str::<Value>(text).ok().and_then(|v| serde_json::to_string_pretty(&v).ok()).unwrap_or_else(|| text.to_string())
}

async fn send(client: &reqwest::Client, spec: &HttpSpec, auth: &[(String, String)], timeout: Duration) -> Result<Response, String> {
    let method = reqwest::Method::from_bytes(spec.method.as_bytes()).map_err(|e| e.to_string())?;
    let mut req = client.request(method, &spec.url).timeout(timeout);
    if !spec.query.is_empty() {
        req = req.query(&spec.query);
    }
    for (k, v) in auth.iter().chain(spec.headers.iter()) {
        req = req.header(k, v);
    }
    req = match &spec.body {
        Some(HttpBody::Json(v)) => req.json(v),
        Some(HttpBody::Form(f)) => req.form(f),
        Some(HttpBody::Text(t)) => req.body(t.clone()),
        None => req,
    };
    let resp = req.send().await.map_err(|e| {
        let mut msg = e.to_string();
        let mut src = std::error::Error::source(&e);
        while let Some(s) = src {
            msg.push_str(&format!(": {s}"));
            src = s.source();
        }
        format!("request failed: {msg}")
    })?;
    let status = resp.status().as_u16();
    let headers = resp.headers().iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string())).collect();
    let text = resp.text().await.map_err(|e| format!("reading response: {e}"))?;
    let json = serde_json::from_str(&text).ok();
    Ok(Response { status, headers, text, json })
}

/// Values at a JSONPath: one node → that value; many → an array; none → None.
pub fn json_at(root: &Value, path: &str) -> Option<Value> {
    let p = serde_json_path::JsonPath::parse(path).ok()?;
    let nodes = p.query(root).all();
    match nodes.len() {
        0 => None,
        1 => Some(nodes[0].clone()),
        _ => Some(Value::Array(nodes.into_iter().cloned().collect())),
    }
}

fn check(spec: &HttpSpec, r: &Response) -> (Vec<Check>, Option<StepError>) {
    let mut checks = Vec::new();
    let mut first_err: Option<StepError> = None;
    let mut fail = |checks: &mut Vec<Check>, what: String, expected: String, actual: String, detail: String| {
        checks.push(Check { what: what.clone(), ok: false, detail: Some(detail.clone()) });
        if first_err.is_none() {
            first_err = Some(StepError { message: format!("{what}: {detail}"), expected: Some(expected), actual: Some(actual) });
        }
    };
    let st = &spec.expect.status;
    let status_ok = if st.is_empty() { (200..300).contains(&r.status) } else { st.contains(&r.status) };
    let want = if st.is_empty() { "2xx".to_string() } else { st.iter().map(u16::to_string).collect::<Vec<_>>().join(" or ") };
    if status_ok {
        checks.push(Check { what: format!("status {}", r.status), ok: true, detail: None });
    } else {
        let body_hint: String = r.text.chars().take(300).collect();
        fail(&mut checks, "status".into(), want.clone(), r.status.to_string(), format!("expected {want}, got {} {}", r.status, body_hint.trim()));
        return (checks, first_err);
    }
    for (path, m) in &spec.expect.json {
        let actual = r.json.as_ref().and_then(|j| json_at(j, path));
        match (&r.json, m.check(actual.as_ref())) {
            (None, _) => fail(&mut checks, path.clone(), m.describe(), "not JSON".into(), "response is not JSON".into()),
            (_, Ok(())) => checks.push(Check { what: format!("{path} {}", m.describe()), ok: true, detail: None }),
            (_, Err(e)) => fail(&mut checks, path.clone(), m.describe(), actual.map(|a| a.to_string()).unwrap_or_else(|| "nothing".into()), e),
        }
    }
    for (name, m) in &spec.expect.headers {
        let actual = r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| Value::String(v.clone()));
        match m.check(actual.as_ref()) {
            Ok(()) => checks.push(Check { what: format!("header {name} {}", m.describe()), ok: true, detail: None }),
            Err(e) => fail(&mut checks, format!("header {name}"), m.describe(), actual.map(|a| a.to_string()).unwrap_or_default(), e),
        }
    }
    if let Some(m) = &spec.expect.body {
        let actual = Value::String(r.text.clone());
        match m.check(Some(&actual)) {
            Ok(()) => checks.push(Check { what: format!("body {}", m.describe()), ok: true, detail: None }),
            Err(e) => fail(&mut checks, "body".into(), m.describe(), r.text.chars().take(200).collect(), e),
        }
    }
    (checks, first_err)
}

fn capture(spec: &HttpSpec, r: &Response) -> Result<Vec<(String, Value)>, StepError> {
    let mut out = Vec::new();
    for (name, c) in &spec.capture {
        let v = match c {
            HttpCapture::Json(path) => r.json.as_ref().and_then(|j| json_at(j, path)),
            HttpCapture::Header(h) => r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(h)).map(|(_, v)| Value::String(v.clone())),
            HttpCapture::Regex(re) => {
                regex::Regex::new(re).ok().and_then(|re| re.captures(&r.text).map(|c| Value::String(c.get(1).or_else(|| c.get(0)).map(|m| m.as_str().to_string()).unwrap_or_default())))
            }
            HttpCapture::Status => Some(Value::from(r.status)),
        };
        match v {
            Some(v) => out.push((name.clone(), v)),
            None => {
                return Err(StepError {
                    message: format!("could not capture `{name}`: nothing found in the response"),
                    expected: Some(format!("{c:?}")),
                    actual: Some(r.text.chars().take(300).collect()),
                });
            }
        }
    }
    Ok(out)
}

pub async fn run_http(client: &reqwest::Client, spec: &HttpSpec, auth: &[(String, String)], masker: &Masker, default_timeout: Duration) -> HttpOutcome {
    let started = Instant::now();
    let timeout = spec.timeout.unwrap_or(Duration::from_secs(30)).max(default_timeout);
    let deadline = spec.eventually.map(|d| started + d);
    let mut polls = Vec::new();
    let request_body = match &spec.body {
        Some(HttpBody::Json(v)) => Some(serde_json::to_string_pretty(v).unwrap_or_default()),
        Some(HttpBody::Form(f)) => Some(f.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")),
        Some(HttpBody::Text(t)) => Some(t.clone()),
        None => None,
    };
    let mut headers: Vec<(String, String)> = auth.iter().chain(spec.headers.iter()).map(|(k, v)| (k.clone(), masker.mask_header(k, v))).collect();
    if matches!(spec.body, Some(HttpBody::Json(_))) {
        headers.push(("content-type".into(), "application/json".into()));
    }
    let mut record = HttpRecord {
        method: spec.method.clone(),
        url: masker.mask(&spec.url),
        request_headers: headers,
        request_body: request_body.map(|b| masker.mask(&truncate(&b))),
        status: None,
        response_headers: vec![],
        response_body: None,
        duration_ms: 0,
        polls: vec![],
        checks: vec![],
    };
    let mut wait = Duration::from_millis(500);
    loop {
        let result = send(client, spec, auth, timeout).await;
        let elapsed = started.elapsed().as_millis() as u64;
        match result {
            Err(e) => {
                record.duration_ms = elapsed;
                let retry = deadline.is_some_and(|d| Instant::now() + wait < d);
                polls.push(Poll { at_ms: elapsed, status: None, summary: masker.mask(&e) });
                if retry {
                    tokio::time::sleep(wait).await;
                    continue;
                }
                record.polls = if spec.eventually.is_some() { polls } else { vec![] };
                return HttpOutcome { record, captured: vec![], error: Some(StepError { message: masker.mask(&e), expected: None, actual: None }) };
            }
            Ok(r) => {
                let (checks, err) = check(spec, &r);
                let summary: String = r.text.chars().take(120).collect::<String>().replace('\n', " ");
                polls.push(Poll { at_ms: elapsed, status: Some(r.status), summary: masker.mask(&summary) });
                let retry = err.is_some() && deadline.is_some_and(|d| Instant::now() + wait < d);
                if retry {
                    tokio::time::sleep(wait).await;
                    wait = (wait * 3 / 2).min(Duration::from_secs(2));
                    continue;
                }
                record.duration_ms = started.elapsed().as_millis() as u64;
                record.status = Some(r.status);
                record.response_headers = r.headers.iter().map(|(k, v)| (k.clone(), masker.mask_header(k, v))).collect();
                record.response_body = Some(masker.mask(&truncate(&pretty(&r.text))));
                record.checks = checks
                    .into_iter()
                    .map(|mut c| {
                        c.what = masker.mask(&c.what);
                        c.detail = c.detail.map(|d| masker.mask(&d));
                        c
                    })
                    .collect();
                if spec.eventually.is_some() {
                    record.polls = polls;
                }
                if let Some(mut e) = err {
                    if spec.eventually.is_some() {
                        e.message = format!("{} (still failing after {:.1}s)", e.message, started.elapsed().as_secs_f32());
                    }
                    e.message = masker.mask(&e.message);
                    e.actual = e.actual.map(|a| masker.mask(&a));
                    return HttpOutcome { record, captured: vec![], error: Some(e) };
                }
                return match capture(spec, &r) {
                    Ok(captured) => HttpOutcome { record, captured, error: None },
                    Err(e) => HttpOutcome { record, captured: vec![], error: Some(e) },
                };
            }
        }
    }
}

/// Used by `eventually` text for matchers in reports.
pub fn describe_matcher(m: &Matcher) -> String {
    m.describe()
}
