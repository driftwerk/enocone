//! Browser steps: run actions through a [`Session`], with auto-waiting
//! assertions and captures.

use std::time::{Duration, Instant};

use enocone_spec::blocks::{Action, BrowserCapture, BrowserExpect, BrowserSpec, By, Locator, UiCheck, WaitSpec};
use enocone_spec::results::{ActionRecord, StepError};
use serde_json::Value;

use crate::driver::{Session, UiAction, UiQuery};
use crate::vars::Masker;

pub struct BrowserOutcome {
    pub records: Vec<ActionRecord>,
    pub captured: Vec<(String, Value)>,
    pub error: Option<StepError>,
    pub wants_screenshot: bool,
}

pub struct Timeouts {
    pub action: Duration,
    pub navigation: Duration,
}

/// Resolve `/path` against a base URL (the env's `WEB`).
pub fn absolute(url: &str, base: Option<&str>) -> String {
    match base {
        Some(b) if url.starts_with('/') => format!("{}{}", b.trim_end_matches('/'), url),
        _ => url.to_string(),
    }
}

/// Driver errors without JavaScript stack frames or repeated lines.
pub fn clean_error(e: &anyhow::Error) -> String {
    let full = format!("{e:#}");
    let mut seen: Vec<String> = vec![];
    for line in full.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("at ") || t.starts_with("=====") {
            continue;
        }
        let t = t.trim_start_matches("TimeoutError: ").trim_start_matches("Error: ").trim_start_matches("Timeout: ").to_string();
        if !seen.iter().any(|x| x.contains(&t) || t.contains(x.as_str())) {
            seen.push(t);
        }
    }
    seen.join("; ")
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn url_matches(actual: &str, want: &str) -> bool {
    if let Some(re) = want.strip_prefix('^').map(|_| want) {
        return regex::Regex::new(re).map(|r| r.is_match(actual)).unwrap_or(false);
    }
    actual.contains(want)
}

async fn check_once(s: &dyn Session, target: &Locator, c: &UiCheck) -> anyhow::Result<Result<(), (String, String)>> {
    let q = |q| s.query(target, q);
    Ok(match c {
        UiCheck::Visible(want) => {
            let v = q(UiQuery::Visible).await?.as_bool().unwrap_or(false);
            if v == *want {
                Ok(())
            } else if let (By::Text { .. }, Some(scope), true) = (&target.by, &target.within, *want) {
                // Say what the area does contain, e.g. the amount actually shown.
                let actual = s.query(scope, UiQuery::Text).await?;
                let shown = match actual.as_str() {
                    Some(t) => format!("{} reads \"{}\"", scope.describe(), norm(t).chars().take(240).collect::<String>()),
                    None => format!("{} not found", scope.describe()),
                };
                Err(("visible".into(), shown))
            } else {
                Err((if *want { "visible" } else { "not visible" }.into(), if v { "visible" } else { "not found or hidden" }.into()))
            }
        }
        UiCheck::Text { text, exact } => {
            let actual = q(UiQuery::Text).await?;
            let a = actual.as_str().map(norm);
            let ok = match &a {
                Some(a) if *exact => a == &norm(text),
                Some(a) => a.to_lowercase().contains(&norm(text).to_lowercase()),
                None => false,
            };
            if ok { Ok(()) } else { Err((format!("text \"{text}\""), a.map(|a| format!("\"{}\"", a.chars().take(300).collect::<String>())).unwrap_or_else(|| "element not found".into()))) }
        }
        UiCheck::Value(want) => {
            let a = q(UiQuery::Value).await?;
            if a.as_str() == Some(want.as_str()) { Ok(()) } else { Err((format!("value \"{want}\""), a.to_string())) }
        }
        UiCheck::Checked(want) => {
            let a = q(UiQuery::Checked).await?;
            if a.as_bool() == Some(*want) {
                Ok(())
            } else {
                Err((
                    if *want { "checked" } else { "not checked" }.into(),
                    if a.is_null() {
                        "element not found".into()
                    } else if a.as_bool() == Some(true) {
                        "checked".into()
                    } else {
                        "not checked".into()
                    },
                ))
            }
        }
        UiCheck::Enabled(want) => {
            let a = q(UiQuery::Enabled).await?;
            if a.as_bool() == Some(*want) {
                Ok(())
            } else {
                Err((
                    if *want { "enabled" } else { "disabled" }.into(),
                    if a.is_null() {
                        "element not found".into()
                    } else if a.as_bool() == Some(true) {
                        "enabled".into()
                    } else {
                        "disabled".into()
                    },
                ))
            }
        }
        UiCheck::Count(n) => {
            let a = q(UiQuery::Count).await?.as_u64().unwrap_or(0) as usize;
            if a == *n { Ok(()) } else { Err((format!("{n} matching"), format!("{a} matching"))) }
        }
        UiCheck::Url(u) => {
            let a = s.url().await?;
            if url_matches(&a, u) { Ok(()) } else { Err((format!("URL matching \"{u}\""), a)) }
        }
        UiCheck::Title(t) => {
            let a = s.title().await?;
            if a.to_lowercase().contains(&t.to_lowercase()) { Ok(()) } else { Err((format!("title containing \"{t}\""), format!("\"{a}\""))) }
        }
    })
}

async fn run_expect(s: &dyn Session, e: &BrowserExpect, timeout: Duration) -> Result<(), StepError> {
    let deadline = Instant::now() + e.timeout.unwrap_or(timeout);
    let mut delay = Duration::from_millis(100);
    loop {
        let mut failure = None;
        for c in &e.checks {
            match check_once(s, &e.target, c).await {
                Ok(Ok(())) => {}
                Ok(Err(ea)) => {
                    failure = Some(ea);
                    break;
                }
                Err(err) => {
                    failure = Some(("a working page".into(), err.to_string()));
                    break;
                }
            }
        }
        match failure {
            None => return Ok(()),
            Some((expected, actual)) if Instant::now() >= deadline => {
                return Err(StepError {
                    message: format!("{}: expected {expected}, found {actual} (waited {:.0}s)", e.target.describe(), e.timeout.unwrap_or(timeout).as_secs_f32()),
                    expected: Some(expected),
                    actual: Some(actual),
                });
            }
            Some(_) => {
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_millis(500));
            }
        }
    }
}

async fn wait_until<F, Fut>(timeout: Duration, mut f: F) -> anyhow::Result<bool>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<bool>>,
{
    let deadline = Instant::now() + timeout;
    loop {
        if f().await? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

async fn run_capture(s: &dyn Session, name: &str, c: &BrowserCapture, timeout: Duration) -> Result<Value, String> {
    match c {
        BrowserCapture::FromUrl(re) => {
            let r = regex::Regex::new(re).map_err(|e| e.to_string())?;
            let deadline = Instant::now() + timeout;
            loop {
                let url = s.url().await.map_err(|e| e.to_string())?;
                if let Some(caps) = r.captures(&url) {
                    let m = caps.name("id").or_else(|| caps.get(1)).or_else(|| caps.get(0)).unwrap();
                    return Ok(Value::String(m.as_str().to_string()));
                }
                if Instant::now() >= deadline {
                    return Err(format!("could not capture `{name}`: URL {url} does not match /{re}/"));
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        BrowserCapture::Text(l) | BrowserCapture::Value(l) | BrowserCapture::Attribute(l, _) => {
            let q = match c {
                BrowserCapture::Text(_) => UiQuery::Text,
                BrowserCapture::Value(_) => UiQuery::Value,
                BrowserCapture::Attribute(_, a) => UiQuery::Attribute(a.clone()),
                _ => unreachable!(),
            };
            let deadline = Instant::now() + timeout;
            loop {
                let v = s.query(l, q.clone()).await.map_err(|e| e.to_string())?;
                if !v.is_null() {
                    return Ok(match v {
                        Value::String(t) => Value::String(t.trim().to_string()),
                        other => other,
                    });
                }
                if Instant::now() >= deadline {
                    return Err(format!("could not capture `{name}`: {} not found", l.describe()));
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

pub async fn run_browser(s: &dyn Session, spec: &BrowserSpec, base: Option<&str>, t: &Timeouts, masker: &Masker) -> BrowserOutcome {
    let mut out = BrowserOutcome { records: vec![], captured: vec![], error: None, wants_screenshot: false };
    for action in &spec.actions {
        let started = Instant::now();
        let desc = masker.mask(&action.describe());
        let result: Result<(), StepError> = async {
            let err = |e: anyhow::Error| {
                let msg = clean_error(&e);
                let msg = if msg.contains("ms exceeded") && !matches!(action, Action::Goto(_)) {
                    format!("{desc}: no matching element was ready within {:.0}s (check the snapshot for the right role and name)", t.action.as_secs_f32())
                } else {
                    format!("{desc}: {msg}")
                };
                StepError { message: masker.mask(&msg), expected: None, actual: None }
            };
            match action {
                Action::Goto(u) => s.goto(&absolute(u, base), t.navigation).await.map_err(err),
                Action::Click(l) => s.act(l, UiAction::Click, t.action).await.map_err(err),
                Action::DblClick(l) => s.act(l, UiAction::DblClick, t.action).await.map_err(err),
                Action::Hover(l) => s.act(l, UiAction::Hover, t.action).await.map_err(err),
                Action::Fill(l, v) => s.act(l, UiAction::Fill(v.clone()), t.action).await.map_err(err),
                Action::Check(l) => s.act(l, UiAction::Check, t.action).await.map_err(err),
                Action::Uncheck(l) => s.act(l, UiAction::Uncheck, t.action).await.map_err(err),
                Action::Select(l, v) => s.act(l, UiAction::Select(v.clone()), t.action).await.map_err(err),
                Action::Upload(l, f) => s.act(l, UiAction::Upload(f.clone()), t.action).await.map_err(err),
                Action::Press(Some(l), k) => s.act(l, UiAction::Press(k.clone()), t.action).await.map_err(err),
                Action::Press(None, k) => s.press(k).await.map_err(err),
                Action::Wait(WaitSpec::Time(d)) => {
                    tokio::time::sleep(*d).await;
                    Ok(())
                }
                Action::Wait(WaitSpec::Url(u)) => {
                    let ok = wait_until(t.navigation, || async { Ok(url_matches(&s.url().await?, u)) }).await.map_err(err)?;
                    if ok {
                        Ok(())
                    } else {
                        Err(StepError { message: format!("URL never matched \"{u}\" (now {})", s.url().await.unwrap_or_default()), expected: Some(u.clone()), actual: s.url().await.ok() })
                    }
                }
                Action::Wait(WaitSpec::Visible(l)) | Action::Wait(WaitSpec::Hidden(l)) => {
                    let want = matches!(action, Action::Wait(WaitSpec::Visible(_)));
                    let ok = wait_until(t.action, || async { Ok(s.query(l, UiQuery::Visible).await?.as_bool().unwrap_or(false) == want) }).await.map_err(err)?;
                    if ok { Ok(()) } else { Err(StepError { message: format!("{} never became {}", l.describe(), if want { "visible" } else { "hidden" }), expected: None, actual: None }) }
                }
                Action::Expect(e) => run_expect(s, e, t.action).await,
                Action::Capture(list) => {
                    for (name, c) in list {
                        let v = run_capture(s, name, c, t.action).await.map_err(|m| StepError { message: m, expected: None, actual: None })?;
                        out.captured.push((name.clone(), v));
                    }
                    Ok(())
                }
                Action::Screenshot => {
                    out.wants_screenshot = true;
                    Ok(())
                }
            }
        }
        .await;
        let ok = result.is_ok();
        out.records.push(ActionRecord { description: desc, ok, duration_ms: started.elapsed().as_millis() as u64, error: result.as_ref().err().map(|e| masker.mask(&e.message)) });
        if let Err(e) = result {
            out.error = Some(StepError { message: masker.mask(&e.message), expected: e.expected.map(|x| masker.mask(&x)), actual: e.actual.map(|x| masker.mask(&x)) });
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn strips_js_stack() {
        let e = anyhow::anyhow!("Timeout: Timeout 5000ms exceeded. \n TimeoutError: Timeout 5000ms exceeded.\n    at _ProgressController.run (/x/coreBundle.js:1:2)\n    at runNext (/x:3)");
        assert_eq!(super::clean_error(&e), "Timeout 5000ms exceeded.");
    }
}
