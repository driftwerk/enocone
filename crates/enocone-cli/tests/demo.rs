//! End-to-end: the `enocone` binary against the fixture app, using the demo
//! project. Set ENOCONE_TEST_NO_BROWSER=1 to skip the browser flows.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_enocone");

fn demo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo").canonicalize().unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("enocone-test-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn browser_tests() -> bool {
    std::env::var("ENOCONE_TEST_NO_BROWSER").is_err()
}

async fn fixture() -> String {
    let (addr, _h) = enocone_fixture::serve("127.0.0.1:0".parse().unwrap()).await;
    format!("http://{addr}")
}

/// Run the CLI off the async runtime (the fixture runs on it).
async fn enocone(project: &Path, base: &str, args: &[&str]) -> (i32, String) {
    let mut cmd = Command::new(BIN);
    cmd.arg("--project").arg(project).args(args).env("ENOCONE_VAR_API", format!("{base}/api")).env("ENOCONE_VAR_WEB", base).env("NO_COLOR", "1");
    let out = tokio::task::spawn_blocking(move || cmd.output().unwrap()).await.unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), text)
}

fn results(dir: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("results.json")).unwrap()).unwrap()
}

fn copy_dir(from: &Path, to: &Path) {
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            if e.file_name() == "enocone-report" || e.file_name() == ".enocone" {
                continue;
            }
            std::fs::create_dir_all(&dest).unwrap();
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn validate_demo() {
    let (code, out) = enocone(&demo(), "http://unused", &["validate"]).await;
    assert_eq!(code, 0, "{out}");
}

#[tokio::test(flavor = "multi_thread")]
async fn http_only_flow_passes() {
    let base = fixture().await;
    let out = scratch("http");
    let (code, log) = enocone(&demo(), &base, &["run", "--id", "leads.agency-refers-lead", "--out", out.to_str().unwrap()]).await;
    assert_eq!(code, 0, "{log}");
    let r = results(&out);
    assert_eq!(r["summary"]["passed"], 1, "{r:#}");
    assert!(out.join("index.html").exists());
    assert!(out.join("junit.xml").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn full_demo_run_and_pending() {
    if !browser_tests() {
        return;
    }
    let base = fixture().await;
    let out = scratch("full");
    let (code, log) = enocone(&demo(), &base, &["run", "--out", out.to_str().unwrap()]).await;
    assert_eq!(code, 0, "{log}");
    let r = results(&out);
    assert_eq!(r["summary"]["passed"], 2, "{log}");
    assert_eq!(r["summary"]["pending"], 2, "{log}");
    assert_eq!(r["summary"]["failed"], 0, "{log}");

    let (code, log) = enocone(&demo(), &base, &["run", "--fail-on-pending", "--out", out.to_str().unwrap()]).await;
    assert_eq!(code, 1, "{log}");
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_amount_fails_with_evidence() {
    if !browser_tests() {
        return;
    }
    let base = fixture().await;
    let proj = scratch("failing");
    copy_dir(&demo(), &proj);
    let flow = proj.join("flows/quotes/client-accepts-and-pays.flow.md");
    let text = std::fs::read_to_string(&flow).unwrap().replace("text: \"£1,368.00\"", "text: \"£1,386.00\"");
    std::fs::write(&flow, text).unwrap();
    let out = proj.join("enocone-report");
    let (code, log) = enocone(&proj, &base, &["run", "--id", "quotes.client-accepts-and-pays", "--out", out.to_str().unwrap()]).await;
    assert_eq!(code, 1, "{log}");
    let r = results(&out);
    let steps = &r["flows"][0]["attempts"][0]["steps"];
    let failed = steps.as_array().unwrap().iter().find(|s| s["status"] == "failed").expect("a failed step");
    assert!(failed["error"]["message"].as_str().unwrap().contains("£1,368.00"), "error should say what was shown: {failed:#}");
    assert!(failed["screenshot"].is_string(), "{failed:#}");
    // Cleanup still ran.
    let cleanup = steps.as_array().unwrap().last().unwrap();
    assert_eq!(cleanup["status"], "passed", "{cleanup:#}");
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_lists_tools_and_validates() {
    let mut child = Command::new(BIN).arg("--project").arg(demo()).arg("mcp").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut rpc = |id: u64, method: &str, params: serde_json::Value| -> serde_json::Value {
        let msg = serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(stdin, "{msg}").unwrap();
        if method == "initialize" {
            let mut line = String::new();
            stdout.read_line(&mut line).unwrap();
            writeln!(stdin, "{}", serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).unwrap();
            return serde_json::from_str(&line).unwrap();
        }
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    let init = rpc(1, "initialize", serde_json::json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }));
    assert_eq!(init["result"]["serverInfo"]["name"], "enocone");
    let tools = rpc(2, "tools/list", serde_json::json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for want in ["project_info", "write_flow", "run_flows", "browser_act", "http_request", "get_failures"] {
        assert!(names.contains(&want), "missing {want}: {names:?}");
    }
    let v = rpc(3, "tools/call", serde_json::json!({ "name": "validate_flow", "arguments": { "content": "# T\n\nWhy.\n\n## When\n\n1. **Go**\n   ```browser\n   - goto: /\n   ```\n" } }));
    let body: serde_json::Value = serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["ok"], true, "{body:#}");
    drop(stdin);
    let _ = child.wait();
}

fn write(dir: &Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

const EXEC_FLOW: &str = r#"# Exec blocks run commands

Why: CLIs are products too.

## When

1. **A command prints JSON**
   ```exec
   run: |
     echo '{"total": 3, "id": "r-42"}'
   expect:
     json: { $.total: 3 }
   capture: { run_id: $.id }
   ```

2. **The captured value reaches the next command, in a scratch folder**
   ```exec
   run: |
     echo "{{run_id}}" > "{{run.tmp}}/out.txt" && cat "{{run.tmp}}/out.txt"
   expect: { stdout: r-42 }
   ```

## Then

3. **A failing command is reported with its exit code**
   ```exec
   run: |
     echo boom >&2; exit 3
   expect: { exit: 3, stderr: boom }
   ```
"#;

#[tokio::test(flavor = "multi_thread")]
async fn exec_blocks_run_and_are_gated() {
    let proj = scratch("exec");
    write(&proj, "enocone.toml", "project = \"exec\"\n[run]\nallow_exec = true\n");
    write(&proj, "flows/exec.flow.md", EXEC_FLOW);
    let out = proj.join("report");
    let (code, log) = enocone(&proj, "http://unused", &["run", "--out", out.to_str().unwrap()]).await;
    assert_eq!(code, 0, "{log}");
    let r = results(&out);
    assert_eq!(r["summary"]["passed"], 1, "{r:#}");
    let steps = &r["flows"][0]["attempts"][0]["steps"];
    assert_eq!(steps[0]["captured"][0][1], "r-42", "{steps:#}");
    assert_eq!(steps[2]["records"][0]["exit"], 3, "{steps:#}");

    // Off by default.
    write(&proj, "enocone.toml", "project = \"exec\"\n");
    let (code, log) = enocone(&proj, "http://unused", &["run", "--out", out.to_str().unwrap()]).await;
    assert_eq!(code, 1, "{log}");
    let r = results(&out);
    let msg = r["flows"][0]["attempts"][0]["steps"][0]["error"]["message"].as_str().unwrap().to_string();
    assert!(msg.contains("allow_exec"), "{msg}");
}

const MAILPIT_FLOW: &str = r#"# Quote email through Mailpit

Checks the Mailpit mailbox reader.

## Given

1. **A quote is created for Kim**
   ```http
   POST: "{{env.API}}/quotes"
   headers: { X-API-Key: agency-key-98765 }
   json: { lead_id: ld-9, client_email: "kim+{{run.short_id}}@example.test", fees: [{ amount: 10000 }] }
   expect: { status: 201 }
   ```

## When

2. **Kim gets the email with a link to accept**
   ```email
   to: "kim+{{run.short_id}}@example.test"
   within: 5s
   expect: { subject: "£100.00", body: Accept it here }
   capture: { link: { link: "/accept/" }, total: { regex: "comes to (£[\\d,]+\\.\\d\\d)" } }
   ```
"#;

#[tokio::test(flavor = "multi_thread")]
async fn email_blocks_read_mailpit() {
    let base = fixture().await;
    let proj = scratch("mailpit");
    write(&proj, "enocone.toml", "project = \"mailpit\"\n[env.local]\nAPI = \"x\"\nWEB = \"x\"\n[email]\nprovider = \"mailpit\"\nurl = \"{{env.WEB}}/mailpit\"\n");
    write(&proj, "flows/mail.flow.md", MAILPIT_FLOW);
    let out = proj.join("report");
    let (code, log) = enocone(&proj, &base, &["run", "--env", "local", "--out", out.to_str().unwrap()]).await;
    assert_eq!(code, 0, "{log}");
    let r = results(&out);
    let step = &r["flows"][0]["attempts"][0]["steps"][1];
    assert_eq!(step["records"][0]["mailbox"], "mailpit", "{step:#}");
    assert!(step["captured"][0][1].as_str().unwrap().contains("/accept/tk_"), "{step:#}");
    assert_eq!(step["captured"][1][1], "£100.00", "{step:#}");

    // No [email]: a clear message.
    write(&proj, "enocone.toml", "project = \"mailpit\"\n[env.local]\nAPI = \"x\"\nWEB = \"x\"\n");
    let (code, _) = enocone(&proj, &base, &["run", "--env", "local", "--out", out.to_str().unwrap()]).await;
    assert_eq!(code, 1);
    let msg = results(&out)["flows"][0]["attempts"][0]["steps"][1]["error"]["message"].as_str().unwrap().to_string();
    assert!(msg.contains("add [email] to enocone.toml"), "{msg}");
}

#[test]
fn old_results_json_still_renders() {
    // A results.json from before designs, exec, email and prototype runs existed.
    let old = r##"{"schema":"enocone.results/v1","run":{"id":"20260101-000000-abcd","project":"old","env":"local","driver":"none","started_at":"2026-01-01T00:00:00Z","duration_ms":5,"enocone_version":"0.1.0","executed":true},
      "summary":{"total":1,"passed":1,"failed":0,"flaky":0,"skipped":0,"pending":0,"steps":1},
      "flows":[{"flow":{"id":"a","title":"A","meta":{"stage":"implemented"},"sections":[{"kind":"when","steps":[{"key":"s","number":1,"intent":"S","blocks":[{"kind":"http","source":"GET: /x","value":{"GET":"/x"},"line":3}],"line":2}]}],"path":"flows/a.flow.md","source_hash":"x","source":"# A"},
        "status":"passed","duration_ms":5,"attempts":[{"status":"passed","duration_ms":5,"steps":[{"key":"s","status":"passed","duration_ms":5,"records":[{"type":"http","method":"GET","url":"/x","status":200,"duration_ms":1}]}]}]}]}"##;
    let dir = scratch("old-results");
    std::fs::write(dir.join("results.json"), old).unwrap();
    let out = dir.join("report");
    let o = Command::new(BIN).args(["report", dir.join("results.json").to_str().unwrap(), "--out", out.to_str().unwrap()]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let html = std::fs::read_to_string(out.join("index.html")).unwrap();
    assert!(html.contains("\"project\":\"old\""), "the data is embedded in the page");
    if !browser_tests() {
        return;
    }
    // The page itself must draw the old run (report.js copes with missing new fields).
    let check = dir.join("check");
    write(&check, "enocone.toml", "project = \"check\"\n");
    let page = format!("file://{}", out.join("index.html").display());
    write(
        &check,
        "flows/renders.flow.md",
        &format!(
            "# The old report renders\n\nIt draws.\n\n## Then\n\n1. **The flow page shows its title and step**\n   ```browser\n   - goto: \"{page}#flow=a\"\n   - expect: {{ heading: A }}\n   - expect: {{ text: Behaved as documented }}\n   - expect: {{ text: GET /x }}\n   ```\n"
        ),
    );
    let o = Command::new(BIN).arg("--project").arg(&check).args(["run", "--out", check.join("report").to_str().unwrap()]).env("NO_COLOR", "1").output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

const REFRESH_FLOW: &str = r#"# Token refresh fails clearly

The sign-in session is gone by the time the token needs refreshing.

## When

1. **The API is called**
   ```http
   GET: "{{env.API}}/short/whoami"
   auth: short
   ```

2. **The token gets old**
   ```exec
   run: sleep 2
   ```

3. **The API is called again**
   ```http
   GET: "{{env.API}}/short/whoami"
   auth: short
   ```
"#;

#[tokio::test(flavor = "multi_thread")]
async fn failed_token_refresh_says_why() {
    if !browser_tests() {
        return;
    }
    let base = fixture().await;
    let proj = scratch("refresh-fails");
    copy_dir(&demo().join("fragments"), &{
        let d = proj.join("fragments");
        std::fs::create_dir_all(&d).unwrap();
        d
    });
    write(
        &proj,
        "enocone.toml",
        "project = \"refresh\"\n[run]\nallow_exec = true\n[env.local]\nAPI = \"x\"\nWEB = \"x\"\n\
         [auth.short]\ntype = \"browser\"\nlogin = \"auth/sign-in\"\nparams = { username = \"alice\", password = \"secret\" }\n\
         token = { url = \"{{env.WEB}}/api/auth/short-session-once\", path = \"$.accessToken\", refresh = \"1s\" }\n",
    );
    write(&proj, "flows/refresh.flow.md", REFRESH_FLOW);
    let out = proj.join("report");
    let (code, log) = enocone(&proj, &base, &["run", "--env", "local", "--out", out.to_str().unwrap()]).await;
    assert_eq!(code, 1, "{log}");
    let steps = &results(&out)["flows"][0]["attempts"][0]["steps"];
    assert_eq!(steps[0]["status"], "passed", "{steps:#}");
    let msg = steps[2]["error"]["message"].as_str().unwrap().to_string();
    assert!(msg.contains("refreshing the token for `short`"), "{msg}");
    assert!(msg.contains("no token at $.accessToken"), "{msg}");
    assert!(msg.contains("saved browser session may have expired"), "{msg}");
    assert!(steps[2]["records"].as_array().is_none_or(|r| r.is_empty()), "no request was sent with a stale token: {steps:#}");
}
