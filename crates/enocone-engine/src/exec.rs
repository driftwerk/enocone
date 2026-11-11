//! Exec steps: run a command, check its exit code and output, capture values.
//! Disabled unless the project sets `run.allow_exec = true`.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use enocone_spec::blocks::{Command, ExecCapture, ExecSpec};
use enocone_spec::results::{Check, ExecRecord, StepError};
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use crate::vars::Masker;

const OUTPUT_LIMIT: usize = 32 * 1024;

pub struct ExecOutcome {
    pub record: ExecRecord,
    pub captured: Vec<(String, Value)>,
    pub error: Option<StepError>,
}

fn tail(s: &str) -> String {
    if s.len() <= OUTPUT_LIMIT {
        return s.to_string();
    }
    let mut start = s.len() - OUTPUT_LIMIT;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    format!("… ({} bytes before)\n{}", start, &s[start..])
}

fn build(spec: &ExecSpec) -> tokio::process::Command {
    match &spec.run {
        Command::Shell(s) => {
            if cfg!(windows) {
                let mut c = tokio::process::Command::new("cmd");
                c.arg("/C").arg(s);
                c
            } else {
                let mut c = tokio::process::Command::new("sh");
                c.arg("-c").arg(s);
                c
            }
        }
        Command::Args(a) => {
            let mut c = tokio::process::Command::new(a.first().map(String::as_str).unwrap_or(""));
            c.args(a.iter().skip(1));
            c
        }
    }
}

pub async fn run_exec(spec: &ExecSpec, root: &Path, masker: &Masker, default_timeout: Duration) -> ExecOutcome {
    let started = Instant::now();
    let cwd = match &spec.cwd {
        Some(c) if Path::new(c).is_absolute() => Path::new(c).to_path_buf(),
        Some(c) => root.join(c),
        None => root.to_path_buf(),
    };
    let mut record =
        ExecRecord { command: masker.mask(&spec.run.display()), cwd: Some(cwd.display().to_string()), exit: None, stdout: String::new(), stderr: String::new(), duration_ms: 0, checks: vec![] };
    let fail = |record: ExecRecord, message: String| ExecOutcome { record, captured: vec![], error: Some(StepError { message, expected: None, actual: None }) };

    let mut cmd = build(spec);
    cmd.current_dir(&cwd).stdin(if spec.stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return fail(record, format!("could not start `{}`: {e}", masker.mask(&spec.run.display()))),
    };
    if let (Some(input), Some(mut stdin)) = (&spec.stdin, child.stdin.take()) {
        let _ = stdin.write_all(input.as_bytes()).await;
    }
    let timeout = spec.timeout.unwrap_or(default_timeout.max(Duration::from_secs(60)));
    let out = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return fail(record, format!("command failed: {e}")),
        Err(_) => {
            record.duration_ms = started.elapsed().as_millis() as u64;
            return fail(record, format!("command did not finish within {}s", timeout.as_secs()));
        }
    };
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let code = out.status.code();
    record.exit = code;
    record.stdout = masker.mask(&tail(&stdout));
    record.stderr = masker.mask(&tail(&stderr));
    record.duration_ms = started.elapsed().as_millis() as u64;

    let mut first_error: Option<StepError> = None;
    let mut check = |record: &mut ExecRecord, what: String, r: Result<(), String>| {
        let ok = r.is_ok();
        record.checks.push(Check { what: what.clone(), ok, detail: r.as_ref().err().map(|e| masker.mask(e)) });
        if let Err(e) = r
            && first_error.is_none()
        {
            first_error = Some(StepError { message: masker.mask(&format!("{what}: {e}")), expected: None, actual: None });
        }
    };

    if let Some(codes) = &spec.expect.exit {
        let r = match code {
            Some(c) if codes.contains(&c) => Ok(()),
            Some(c) => {
                let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
                Err(format!("expected {}, got {c}{}", codes.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" or "), if last.is_empty() { String::new() } else { format!(" — {last}") }))
            }
            None => Err("killed by a signal".into()),
        };
        check(&mut record, "exit code".into(), r);
    }
    if let Some(m) = &spec.expect.stdout {
        check(&mut record, format!("stdout {}", m.describe()), m.check(Some(&Value::String(stdout.clone()))));
    }
    if let Some(m) = &spec.expect.stderr {
        check(&mut record, format!("stderr {}", m.describe()), m.check(Some(&Value::String(stderr.clone()))));
    }
    let parsed: Option<Value> = serde_json::from_str(stdout.trim()).ok();
    for (path, m) in &spec.expect.json {
        let r = match &parsed {
            None => Err("stdout is not JSON".to_string()),
            Some(j) => m.check(crate::http::json_for(j, path, m).as_ref()),
        };
        check(&mut record, format!("{path} {}", m.describe()), r);
    }

    let mut captured = vec![];
    if first_error.is_none() {
        for (name, c) in &spec.capture {
            let v = match c {
                ExecCapture::Exit => code.map(Value::from),
                ExecCapture::Json(p) => parsed.as_ref().and_then(|j| crate::http::capture_at(j, p)),
                ExecCapture::Regex(r) => regex::Regex::new(r).ok().and_then(|re| re.captures(&stdout)).and_then(|c| c.get(1).or_else(|| c.get(0))).map(|m| Value::String(m.as_str().to_string())),
            };
            match v {
                Some(v) => captured.push((name.clone(), v)),
                None => {
                    first_error = Some(StepError { message: format!("could not capture `{name}` from the output"), expected: None, actual: None });
                    break;
                }
            }
        }
    }
    ExecOutcome { record, captured, error: first_error }
}
