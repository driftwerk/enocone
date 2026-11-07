//! One renderer for docs and reports: the flow documents, augmented with
//! run results when there are any.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use enocone_spec::results::{RunResults, Status};

const CSS: &str = include_str!("../assets/report.css");
const JS: &str = include_str!("../assets/report.js");

pub struct ReportOptions {
    /// Embed screenshots as data URIs so the HTML is a single file.
    pub inline_assets: bool,
}

/// Render the HTML page for `results` (screenshots resolved against `assets_root`).
pub fn render_html(results: &RunResults, assets_root: &Path, opts: &ReportOptions) -> anyhow::Result<String> {
    let mut data = results.clone();
    if opts.inline_assets {
        for fr in &mut data.flows {
            for a in &mut fr.attempts {
                for s in &mut a.steps {
                    if let Some(p) = &s.screenshot
                        && let Ok(bytes) = std::fs::read(assets_root.join(p))
                    {
                        s.screenshot = Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)));
                    }
                }
            }
        }
    }
    // Make the JSON safe inside <script>.
    let json = serde_json::to_string(&data)?.replace("</", "<\\/").replace("<!--", "<\\!--");
    let title = if results.run.executed { format!("{} · enocone report", results.run.project) } else { format!("{} · flow documentation", results.run.project) };
    let mut html = String::with_capacity(json.len() + CSS.len() + JS.len() + 1024);
    html.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = writeln!(html, "<title>{}</title>", escape(&title));
    // Fonts load when online; offline the report falls back to Georgia and the system sans.
    html.push_str("<link rel=\"preconnect\" href=\"https://fonts.googleapis.com\">\n<link rel=\"preconnect\" href=\"https://fonts.gstatic.com\" crossorigin>\n<link rel=\"stylesheet\" href=\"https://fonts.googleapis.com/css2?family=Fraunces:ital,opsz,wght@0,9..144,400;0,9..144,600;1,9..144,400&family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap\">\n");
    html.push_str("<meta name=\"generator\" content=\"enocone\">\n<style>\n");
    html.push_str(CSS);
    html.push_str("\n</style>\n</head>\n<body>\n<div id=\"app\"><noscript>This report needs JavaScript. The raw data is in results.json.</noscript></div>\n");
    html.push_str("<script type=\"application/json\" id=\"enocone-data\">");
    html.push_str(&json);
    html.push_str("</script>\n<script>\n");
    html.push_str(JS);
    html.push_str("\n</script>\n</body>\n</html>\n");
    Ok(html)
}

/// Write `<dir>/index.html` and return its path.
pub fn write_html(results: &RunResults, dir: &Path, opts: &ReportOptions) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let html = render_html(results, dir, opts)?;
    let path = dir.join("index.html");
    std::fs::write(&path, html)?;
    Ok(path)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// JUnit XML: one test suite per area, one test case per flow.
pub fn junit(results: &RunResults) -> String {
    let mut areas: Vec<String> = results.flows.iter().map(|f| f.flow.area().to_string()).collect();
    areas.sort();
    areas.dedup();
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        x,
        "<testsuites name=\"{}\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" time=\"{:.3}\">",
        escape(&results.run.project),
        results.summary.total,
        results.summary.failed,
        results.summary.skipped + results.summary.pending,
        results.run.duration_ms as f64 / 1000.0
    );
    for area in areas {
        let flows: Vec<_> = results.flows.iter().filter(|f| f.flow.area() == area).collect();
        let failures = flows.iter().filter(|f| f.status == Status::Failed).count();
        let skipped = flows.iter().filter(|f| matches!(f.status, Status::Skipped | Status::Pending | Status::NotRun)).count();
        let time: u64 = flows.iter().map(|f| f.duration_ms).sum();
        let _ = writeln!(x, "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{failures}\" skipped=\"{skipped}\" time=\"{:.3}\">", escape(&area), flows.len(), time as f64 / 1000.0);
        for f in flows {
            let _ = write!(x, "    <testcase classname=\"{}\" name=\"{}\" file=\"{}\" time=\"{:.3}\"", escape(&area), escape(&f.flow.title), escape(&f.flow.path), f.duration_ms as f64 / 1000.0);
            match f.status {
                Status::Failed => {
                    let attempt = f.attempts.last();
                    let failed = attempt.and_then(|a| a.steps.iter().find(|s| s.status == Status::Failed));
                    let step = failed.and_then(|r| f.flow.steps().find(|s| s.key == r.key));
                    let msg = failed.and_then(|r| r.error.as_ref()).map(|e| e.message.clone()).unwrap_or_else(|| "failed".into());
                    let what = step.map(|s| format!("Step {}: {}", s.number.map(|n| n.to_string()).unwrap_or_default(), s.intent)).unwrap_or_default();
                    let _ = writeln!(x, ">\n      <failure message=\"{}\">{}\n{}</failure>\n    </testcase>", escape(&msg), escape(&what), escape(&msg));
                }
                Status::Skipped | Status::Pending | Status::NotRun => {
                    let why = f.skip_reason.clone().unwrap_or_else(|| "pending: not fully implemented".into());
                    let _ = writeln!(x, ">\n      <skipped message=\"{}\"/>\n    </testcase>", escape(&why));
                }
                _ => {
                    let _ = writeln!(x, "/>");
                }
            }
        }
        x.push_str("  </testsuite>\n");
    }
    x.push_str("</testsuites>\n");
    x
}

/// A short Markdown summary for `$GITHUB_STEP_SUMMARY` or a PR comment.
pub fn markdown_summary(results: &RunResults, report_link: Option<&str>) -> String {
    let s = &results.summary;
    let mut m = String::new();
    let icon = if s.failed > 0 {
        "❌"
    } else if s.pending > 0 {
        "🟣"
    } else {
        "✅"
    };
    let _ = writeln!(m, "### {icon} enocone · {} · {}\n", results.run.project, results.run.env);
    let _ = writeln!(m, "| Flows | Passed | Failed | Flaky | Pending | Skipped | Time |\n|---|---|---|---|---|---|---|");
    let _ = writeln!(m, "| {} | {} | {} | {} | {} | {} | {:.0}s |\n", s.total, s.passed, s.failed, s.flaky, s.pending, s.skipped, results.run.duration_ms as f64 / 1000.0);
    let failed: Vec<_> = results.flows.iter().filter(|f| f.status == Status::Failed).collect();
    if !failed.is_empty() {
        m.push_str("**Failed**\n\n");
        for f in failed {
            let r = f.attempts.last().and_then(|a| a.steps.iter().find(|s| s.status == Status::Failed));
            let step = r.and_then(|r| f.flow.steps().find(|s| s.key == r.key));
            let _ = writeln!(
                m,
                "- **{}** — step {}: {}  \n  `{}`",
                f.flow.title,
                step.and_then(|s| s.number).map(|n| n.to_string()).unwrap_or_default(),
                step.map(|s| s.intent.as_str()).unwrap_or("cleanup"),
                r.and_then(|r| r.error.as_ref()).map(|e| e.message.replace('`', "'")).unwrap_or_default()
            );
        }
        m.push('\n');
    }
    if let Some(link) = report_link {
        let _ = writeln!(m, "[Open the full report]({link})");
    }
    m
}
