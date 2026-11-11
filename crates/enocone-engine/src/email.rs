//! Email blocks: wait for a message in a sandbox mailbox, check it and
//! capture from it. Mailboxes: Postmark (a sandbox server's Messages API),
//! Mailpit, and MailHog.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use enocone_spec::blocks::{EmailCapture, EmailSpec};
use enocone_spec::results::{Check, EmailRecord, StepError};
use regex::Regex;
use serde_json::Value;

use crate::vars::Masker;

/// `[email]` in enocone.toml, with templates already rendered.
#[derive(Debug, Clone)]
pub struct Mailbox {
    pub provider: Provider,
    pub url: String,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Provider {
    Postmark,
    Mailpit,
    Mailhog,
}

impl Provider {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "postmark" => Ok(Self::Postmark),
            "mailpit" => Ok(Self::Mailpit),
            "mailhog" => Ok(Self::Mailhog),
            other => Err(format!("unknown email provider `{other}` (postmark, mailpit or mailhog)")),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Postmark => "postmark",
            Self::Mailpit => "mailpit",
            Self::Mailhog => "mailhog",
        }
    }
    pub fn default_url(self) -> &'static str {
        match self {
            Self::Postmark => "https://api.postmarkapp.com",
            Self::Mailpit | Self::Mailhog => "http://127.0.0.1:8025",
        }
    }
}

#[derive(Debug, Clone)]
struct Summary {
    id: String,
    subject: String,
    from: String,
    received: Option<DateTime<Utc>>,
    /// MailHog includes the body in the listing.
    inline: Option<(String, String)>,
}

pub struct EmailOutcome {
    pub record: EmailRecord,
    pub captured: Vec<(String, Value)>,
    pub error: Option<StepError>,
}

/// Messages a little older than the flow's start still count (clocks differ).
const CLOCK_SLACK: chrono::TimeDelta = chrono::TimeDelta::seconds(5);

pub async fn run_email(http: &reqwest::Client, mailbox: &Mailbox, spec: &EmailSpec, since: DateTime<Utc>, masker: &Masker) -> EmailOutcome {
    let started = Instant::now();
    let within = spec.within.unwrap_or(Duration::from_secs(30));
    let mut record = EmailRecord {
        to: spec.to.clone(),
        mailbox: mailbox.provider.as_str().into(),
        subject: None,
        from: None,
        received_at: None,
        text: String::new(),
        links: vec![],
        seen: vec![],
        duration_ms: 0,
        checks: vec![],
    };
    let fail = |mut record: EmailRecord, msg: String| {
        record.duration_ms = started.elapsed().as_millis() as u64;
        EmailOutcome { error: Some(StepError { message: masker.mask(&msg), expected: None, actual: None }), record, captured: vec![] }
    };

    // Wait for the newest message to the address, sent after the flow started.
    let mut last_list: Vec<Summary> = vec![];
    let found = loop {
        match list(http, mailbox, &spec.to).await {
            Ok(all) => {
                let fresh = all
                    .iter()
                    .filter(|m| is_fresh(m.received, since))
                    .filter(|m| spec.subject.as_ref().is_none_or(|s| m.subject.to_lowercase().contains(&s.to_lowercase())))
                    .max_by_key(|m| m.received)
                    .cloned();
                last_list = all;
                if let Some(m) = fresh {
                    break m;
                }
            }
            Err(e) => return fail(record, format!("could not read the {} mailbox at {}: {e}", mailbox.provider.as_str(), mailbox.url)),
        }
        if started.elapsed() >= within {
            record.seen = last_list
                .iter()
                .take(10)
                .map(|m| {
                    let when = m.received.map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)).unwrap_or_default();
                    let old = if m.received.is_some_and(|t| t + CLOCK_SLACK < since) { " (before this flow started)" } else { "" };
                    format!("{when} “{}”{old}", m.subject)
                })
                .collect();
            let what = spec.subject.as_ref().map(|s| format!(" with a subject containing “{s}”")).unwrap_or_default();
            let had = if last_list.is_empty() {
                "the mailbox has nothing for that address".to_string()
            } else {
                format!("the mailbox has {} for that address: {}", plural(last_list.len()), record.seen.join("; "))
            };
            return fail(record, format!("no email to {}{what} arrived within {}s; {had}", spec.to, within.as_secs()));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };

    // A mailbox can list a message a moment before it serves its details:
    // "not found" then means "not ready yet", until the step's time runs out.
    let (text, html) = match &found.inline {
        Some(b) => b.clone(),
        None => loop {
            match body(http, mailbox, &found.id).await {
                Ok(b) => break b,
                Err(e) if e.not_found() && started.elapsed() < within => tokio::time::sleep(Duration::from_millis(500)).await,
                Err(e) if e.not_found() => {
                    return fail(record, format!("message {} was listed but never readable within {}s: {e}", found.id, within.as_secs()));
                }
                Err(e) => return fail(record, format!("could not read message {}: {e}", found.id)),
            }
        },
    };
    let text = if text.trim().is_empty() { strip_tags(&html) } else { text };
    let links = links_in(&html, &text);
    record.subject = Some(masker.mask(&found.subject));
    record.from = Some(masker.mask(&found.from));
    record.received_at = found.received.map(|t| t.to_rfc3339());
    record.text = masker.mask(&shorten(&text, 4000));
    record.links = links.iter().map(|l| masker.mask(l)).collect();

    let mut first_error: Option<String> = None;
    let mut check = |record: &mut EmailRecord, what: String, r: Result<(), String>| {
        let ok = r.is_ok();
        let detail = r.err();
        if let Some(d) = &detail
            && first_error.is_none()
        {
            first_error = Some(format!("{what}: {d}"));
        }
        record.checks.push(Check { what, ok, detail: detail.map(|d| masker.mask(&d)) });
    };
    if let Some(m) = &spec.expect.subject {
        check(&mut record, format!("subject {}", m.describe()), m.check(Some(&Value::String(found.subject.clone()))));
    }
    if let Some(m) = &spec.expect.from {
        check(&mut record, format!("from {}", m.describe()), m.check(Some(&Value::String(found.from.clone()))));
    }
    if let Some(m) = &spec.expect.body {
        check(&mut record, format!("body {}", m.describe()), m.check(Some(&Value::String(text.clone()))));
    }
    let mut captured = vec![];
    for (name, c) in &spec.capture {
        let v = match c {
            EmailCapture::Subject => Some(found.subject.clone()),
            EmailCapture::Link(part) => links.iter().find(|l| l.contains(part.as_str())).cloned(),
            EmailCapture::Regex(r) => Regex::new(r).ok().and_then(|re| re.captures(&text)).and_then(|c| c.get(1).or_else(|| c.get(0))).map(|m| m.as_str().to_string()),
        };
        match v {
            Some(v) => captured.push((name.clone(), Value::String(v))),
            None => {
                let what = match c {
                    EmailCapture::Link(p) if p.is_empty() => "a link".to_string(),
                    EmailCapture::Link(p) => format!("a link containing “{p}”"),
                    EmailCapture::Regex(r) => format!("text matching /{r}/"),
                    EmailCapture::Subject => "a subject".into(),
                };
                let seen = if links.is_empty() { "it has no links".to_string() } else { format!("its links are: {}", links.join(", ")) };
                check(&mut record, format!("capture {name}"), Err(format!("the email has no {what}; {seen}")));
            }
        }
    }
    record.duration_ms = started.elapsed().as_millis() as u64;
    let error = first_error.map(|m| StepError { message: masker.mask(&m), expected: None, actual: None });
    EmailOutcome { record, captured, error }
}

/// Provider timestamps: Postmark `2014-02-20T07:25:02.8782715-05:00`, Mailpit and
/// MailHog RFC 3339 (MailHog with nanoseconds).
fn parse_time(v: &Value) -> Option<DateTime<Utc>> {
    v.as_str().and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&Utc))
}

/// Was this message sent after the flow started (allowing for clock skew)?
fn is_fresh(received: Option<DateTime<Utc>>, since: DateTime<Utc>) -> bool {
    received.is_none_or(|t| t + CLOCK_SLACK >= since)
}

fn plural(n: usize) -> String {
    if n == 1 { "1 message".into() } else { format!("{n} messages") }
}

/// A failed mailbox request: its HTTP status (when there was one) and why.
#[derive(Debug)]
struct MailError {
    status: Option<u16>,
    message: String,
}

impl MailError {
    /// The mailbox doesn't have it (yet): 404, or Postmark's 422 with ErrorCode 701.
    fn not_found(&self) -> bool {
        match self.status {
            Some(404) => true,
            Some(422) => {
                let body = self.message.split_once(": ").map(|(_, b)| b).unwrap_or("");
                serde_json::from_str::<Value>(body).is_ok_and(|v| v["ErrorCode"] == 701) || body.replace(' ', "").contains("\"ErrorCode\":701")
            }
            _ => false,
        }
    }
}

impl std::fmt::Display for MailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<String> for MailError {
    fn from(message: String) -> Self {
        MailError { status: None, message }
    }
}

impl From<&str> for MailError {
    fn from(message: &str) -> Self {
        MailError { status: None, message: message.into() }
    }
}

async fn get_json(http: &reqwest::Client, mailbox: &Mailbox, url: &str) -> Result<Value, MailError> {
    let mut req = http.get(url).header("Accept", "application/json");
    match (mailbox.provider, &mailbox.token) {
        (Provider::Postmark, Some(t)) => req = req.header("X-Postmark-Server-Token", t),
        (Provider::Postmark, None) => return Err("Postmark needs `token` (a sandbox server's API token) under [email]".into()),
        (_, Some(t)) => {
            req = match t.split_once(':') {
                Some((u, p)) => req.basic_auth(u, Some(p)),
                None => req.bearer_auth(t),
            }
        }
        (_, None) => {}
    }
    let resp = req.timeout(Duration::from_secs(15)).send().await.map_err(|e| e.without_url().to_string())?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(MailError { status: Some(status.as_u16()), message: format!("HTTP {status}: {}", shorten(&text, 300)) });
    }
    serde_json::from_str(&text).map_err(|e| format!("not JSON: {e}").into())
}

fn base(mailbox: &Mailbox) -> &str {
    mailbox.url.trim_end_matches('/')
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

async fn list(http: &reqwest::Client, mailbox: &Mailbox, to: &str) -> Result<Vec<Summary>, MailError> {
    let s = |v: &Value| v.as_str().unwrap_or("").to_string();
    let time = parse_time;
    match mailbox.provider {
        Provider::Postmark => {
            let v = get_json(http, mailbox, &format!("{}/messages/outbound?recipient={}&count=50&offset=0", base(mailbox), enc(to))).await?;
            Ok(v["Messages"]
                .as_array()
                .map(|a| a.iter().map(|m| Summary { id: s(&m["MessageID"]), subject: s(&m["Subject"]), from: s(&m["From"]), received: time(&m["ReceivedAt"]), inline: None }).collect())
                .unwrap_or_default())
        }
        Provider::Mailpit => {
            let q = format!("to:\"{to}\"");
            let v = get_json(http, mailbox, &format!("{}/api/v1/search?query={}&limit=50", base(mailbox), enc(&q))).await?;
            Ok(v["messages"]
                .as_array()
                .map(|a| a.iter().map(|m| Summary { id: s(&m["ID"]), subject: s(&m["Subject"]), from: s(&m["From"]["Address"]), received: time(&m["Created"]), inline: None }).collect())
                .unwrap_or_default())
        }
        Provider::Mailhog => {
            let v = get_json(http, mailbox, &format!("{}/api/v2/search?kind=to&query={}&limit=50", base(mailbox), enc(to))).await?;
            Ok(v["items"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|m| {
                            let hdr = |k: &str| m["Content"]["Headers"][k][0].as_str().unwrap_or("").to_string();
                            Summary { id: s(&m["ID"]), subject: hdr("Subject"), from: hdr("From"), received: time(&m["Created"]), inline: Some(mailhog_body(m)) }
                        })
                        .collect()
                })
                .unwrap_or_default())
        }
    }
}

/// (text, html) of one message.
async fn body(http: &reqwest::Client, mailbox: &Mailbox, id: &str) -> Result<(String, String), MailError> {
    let s = |v: &Value| v.as_str().unwrap_or("").to_string();
    match mailbox.provider {
        Provider::Postmark => {
            let v = get_json(http, mailbox, &format!("{}/messages/outbound/{}/details", base(mailbox), enc(id))).await?;
            Ok((s(&v["TextBody"]), s(&v["HtmlBody"])))
        }
        Provider::Mailpit => {
            let v = get_json(http, mailbox, &format!("{}/api/v1/message/{}", base(mailbox), enc(id))).await?;
            Ok((s(&v["Text"]), s(&v["HTML"])))
        }
        Provider::Mailhog => Err("MailHog bodies come with the listing".into()),
    }
}

fn mailhog_body(m: &Value) -> (String, String) {
    let decode = |part: &Value| {
        let body = part["Body"].as_str().unwrap_or("");
        let enc = part["Headers"]["Content-Transfer-Encoding"][0].as_str().unwrap_or("").to_ascii_lowercase();
        if enc == "quoted-printable" { quoted_printable(body) } else { body.to_string() }
    };
    let (mut text, mut html) = (String::new(), String::new());
    if let Some(parts) = m["MIME"]["Parts"].as_array() {
        for p in parts {
            let ct = p["Headers"]["Content-Type"][0].as_str().unwrap_or("").to_ascii_lowercase();
            if ct.starts_with("text/html") && html.is_empty() {
                html = decode(p);
            } else if ct.starts_with("text/plain") && text.is_empty() {
                text = decode(p);
            }
        }
    }
    if text.is_empty() && html.is_empty() {
        let c = &m["Content"];
        let ct = c["Headers"]["Content-Type"][0].as_str().unwrap_or("").to_ascii_lowercase();
        let b = decode(c);
        if ct.starts_with("text/html") { html = b } else { text = b }
    }
    (text, html)
}

fn quoted_printable(s: &str) -> String {
    let s = s.replace("=\r\n", "").replace("=\n", "");
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'='
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">")
}

fn strip_tags(html: &str) -> String {
    let no_style = Regex::new(r"(?is)<(style|script)[^>]*>.*?</(style|script)>").unwrap().replace_all(html, " ");
    let breaks = Regex::new(r"(?i)<br\s*/?>|</p>|</div>|</h\d>|</li>|</tr>").unwrap().replace_all(&no_style, "\n");
    let text = Regex::new(r"<[^>]+>").unwrap().replace_all(&breaks, "");
    let text = unescape(&text);
    let lines: Vec<String> = text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect();
    lines.join("\n")
}

/// Links in the order they appear: `href`s from the HTML, then bare URLs in the text.
fn links_in(html: &str, text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let href = Regex::new(r#"(?i)href\s*=\s*["']([^"']+)["']"#).unwrap();
    for c in href.captures_iter(html) {
        let l = unescape(&c[1]);
        if !l.starts_with("mailto:") && !out.contains(&l) {
            out.push(l);
        }
    }
    let bare = Regex::new(r#"https?://[^\s<>"')\]]+"#).unwrap();
    for m in bare.find_iter(text) {
        let l = m.as_str().trim_end_matches(['.', ',', ';']).to_string();
        if !out.contains(&l) {
            out.push(l);
        }
    }
    out
}

fn shorten(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_links_and_text() {
        let html = r#"<style>p{}</style><p>Hello Jane,</p><p><a href="https://app.example.test/public/quotes/q1/accept/tk&amp;x=1">Accept</a> or <a href="mailto:help@example.test">ask</a></p>"#;
        let links = links_in(html, "Or paste https://app.example.test/other.");
        assert_eq!(links, vec!["https://app.example.test/public/quotes/q1/accept/tk&x=1", "https://app.example.test/other"]);
        assert_eq!(strip_tags(html), "Hello Jane,\nAccept or ask");
        assert_eq!(quoted_printable("Total =C2=A31,450=\n.00"), "Total £1,450.00");
    }

    #[test]
    fn real_provider_timestamps() {
        // Postmark's documented format: 7 fractional digits and a US Eastern offset.
        let pm = parse_time(&Value::String("2014-02-20T07:25:02.8782715-05:00".into())).unwrap();
        assert_eq!(pm.to_rfc3339_opts(chrono::SecondsFormat::Secs, true), "2014-02-20T12:25:02Z");
        let since = DateTime::parse_from_rfc3339("2014-02-20T12:25:05Z").unwrap().with_timezone(&Utc);
        assert!(is_fresh(Some(pm), since), "3s before the flow started is within the clock slack");
        let later = DateTime::parse_from_rfc3339("2014-02-20T12:26:00Z").unwrap().with_timezone(&Utc);
        assert!(!is_fresh(Some(pm), later), "a minute old: from an earlier run");
        // MailHog marshals Go's time.Time with nanoseconds.
        assert!(parse_time(&Value::String("2026-11-06T20:10:29.123456789+01:00".into())).is_some());
    }

    #[test]
    fn mailhog_multipart_with_quoted_printable() {
        let item: Value = serde_json::from_str(
            r#"{
          "ID": "abc@mailhog.example", "Created": "2026-11-06T20:10:29.123456789Z",
          "Content": { "Headers": { "Subject": ["Your quote"], "Content-Type": ["multipart/alternative; boundary=x"] }, "Body": "raw" },
          "MIME": { "Parts": [
            { "Headers": { "Content-Type": ["text/plain; charset=UTF-8"], "Content-Transfer-Encoding": ["quoted-printable"] }, "Body": "Total =C2=A3100.00" },
            { "Headers": { "Content-Type": ["text/html; charset=UTF-8"] }, "Body": "<a href=\"https://x.test/accept/1\">Accept</a>" }
          ] }
        }"#,
        )
        .unwrap();
        let (text, html) = mailhog_body(&item);
        assert_eq!(text, "Total £100.00");
        assert_eq!(links_in(&html, &text), vec!["https://x.test/accept/1"]);
        // Single part: MIME is null.
        let single: Value = serde_json::from_str(r#"{ "Content": { "Headers": { "Content-Type": ["text/plain"] }, "Body": "Hi https://x.test/a" }, "MIME": null }"#).unwrap();
        assert_eq!(mailhog_body(&single).0, "Hi https://x.test/a");
    }

    /// A Postmark stand-in that lists one message, whose details answer
    /// `detail_status` for the first `misses` reads (and every read when
    /// `misses` is `usize::MAX`), then the message.
    async fn mailbox(misses: usize, detail_status: u16) -> (Mailbox, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::get};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let reads = std::sync::Arc::new(AtomicUsize::new(0));
        let r2 = reads.clone();
        let app = Router::new()
            .route(
                "/messages/outbound",
                get(|| async { Json(serde_json::json!({ "Messages": [{ "MessageID": "m1", "Subject": "Welcome", "From": "a@x.test", "ReceivedAt": Utc::now().to_rfc3339() }] })) }),
            )
            .route(
                "/messages/outbound/{id}/details",
                get(move || {
                    let n = r2.fetch_add(1, Ordering::SeqCst);
                    async move {
                        if n < misses {
                            let code = if detail_status == 422 { 701 } else { 0 };
                            (StatusCode::from_u16(detail_status).unwrap(), Json(serde_json::json!({ "ErrorCode": code, "Message": "This message was not found." }))).into_response()
                        } else {
                            Json(serde_json::json!({ "TextBody": "Hello", "HtmlBody": "" })).into_response()
                        }
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (Mailbox { provider: Provider::Postmark, url: format!("http://{addr}"), token: Some("t".into()) }, reads)
    }

    fn spec(within_ms: u64) -> EmailSpec {
        EmailSpec { to: "jane@x.test".into(), subject: None, within: Some(Duration::from_millis(within_ms)), expect: Default::default(), capture: vec![] }
    }

    #[tokio::test]
    async fn a_listed_message_is_read_once_its_details_are_ready() {
        let (mb, reads) = mailbox(2, 422).await;
        let out = run_email(&reqwest::Client::new(), &mb, &spec(5000), Utc::now() - chrono::TimeDelta::seconds(1), &Masker::default()).await;
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.record.text, "Hello");
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_message_that_never_becomes_readable_fails_when_time_runs_out() {
        let (mb, _) = mailbox(usize::MAX, 422).await;
        let out = run_email(&reqwest::Client::new(), &mb, &spec(1200), Utc::now() - chrono::TimeDelta::seconds(1), &Masker::default()).await;
        let e = out.error.expect("fails").message;
        assert!(e.contains("message m1 was listed but never readable within 1s"), "{e}");
    }

    #[tokio::test]
    async fn other_errors_fail_at_once() {
        let (mb, reads) = mailbox(usize::MAX, 500).await;
        let out = run_email(&reqwest::Client::new(), &mb, &spec(5000), Utc::now() - chrono::TimeDelta::seconds(1), &Masker::default()).await;
        let e = out.error.expect("fails").message;
        assert!(e.contains("could not read message m1: HTTP 500"), "{e}");
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
