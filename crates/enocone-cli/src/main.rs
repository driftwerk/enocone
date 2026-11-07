#![allow(clippy::format_in_format_args)] // coloured summary line reads better inline
//! `enocone`: business flow documents that run as end-to-end tests.

mod init;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use enocone_engine::{Event, Project};
use enocone_mcp::app::{self, RunError, RunRequest};
use enocone_spec::{Diagnostic, FlowResult, Severity, Status};

#[derive(Parser)]
#[command(name = "enocone", version, about = "Business flow documents that run as end-to-end tests", long_about = None)]
struct Cli {
    /// Project directory (default: search upwards from the current directory for enocone.toml).
    #[arg(long, global = true)]
    project: Option<PathBuf>,
    /// Load secrets from a dotenv file (default: .enocone.env next to enocone.toml, if present).
    #[arg(long, global = true)]
    env_file: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Cmd {
    /// Run flows against an environment and write the report.
    Run(RunArgs),
    /// Render the flow documents (no run) as a readable page.
    Docs {
        paths: Vec<PathBuf>,
        /// Output folder (default: enocone-docs/ in the project).
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        open: bool,
    },
    /// Check flow documents for mistakes.
    Validate {
        paths: Vec<PathBuf>,
        /// Print diagnostics as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Re-render a report from a results.json.
    Report {
        results: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        open: bool,
    },
    /// Draw a mockup screen with the project's theme and save it as a PNG.
    RenderScreen {
        /// A screen name (screens/<name>.html) or a path to an .html file.
        screen: String,
        /// Where to write the PNG (default: .enocone/screens/<name>.png in the project).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Serve the flows' screens as a clickable prototype (storyboards link them).
    Prototype {
        paths: Vec<PathBuf>,
        #[arg(long, default_value_t = 4020)]
        port: u16,
        #[arg(long)]
        open: bool,
    },
    /// List flows with their stage and status of implementation.
    List { paths: Vec<PathBuf> },
    /// Start the MCP server on stdio (for Claude Code, Claude Desktop, Cursor…).
    Mcp,
    /// Create enocone.toml, an example flow and agent config in this directory.
    Init {
        #[arg(long, default_value = "my-app")]
        name: String,
    },
    /// Install the browsers matching the bundled Playwright version.
    InstallBrowsers {
        /// chromium, firefox, webkit (default: chromium)
        browsers: Vec<String>,
        /// Also install system libraries (Linux CI).
        #[arg(long)]
        with_deps: bool,
    },
}

#[derive(clap::Args)]
struct RunArgs {
    /// Flow files or directories (default: the project's flows directory).
    paths: Vec<PathBuf>,
    #[arg(long, short)]
    env: Option<String>,
    /// Only flows with this tag (repeatable).
    #[arg(long = "tag", short)]
    tags: Vec<String>,
    /// Only flows in this area (repeatable).
    #[arg(long = "area")]
    areas: Vec<String>,
    /// Only these flow ids (or id prefixes like `quotes`).
    #[arg(long = "id")]
    ids: Vec<String>,
    /// Only flows whose title or id contains this text.
    #[arg(long)]
    grep: Option<String>,
    #[arg(long, short)]
    workers: Option<usize>,
    #[arg(long)]
    retries: Option<u32>,
    /// Show the browser.
    #[arg(long)]
    headed: bool,
    /// Slow each browser action down (ms), handy with --headed.
    #[arg(long)]
    slow_mo: Option<f64>,
    /// chromium, firefox or webkit.
    #[arg(long)]
    browser: Option<String>,
    /// Use an installed browser channel, e.g. `chrome`.
    #[arg(long)]
    channel: Option<String>,
    /// Output folder (default: enocone-report/ in the project, where the MCP looks).
    #[arg(long)]
    out: Option<PathBuf>,
    /// Open the report when done.
    #[arg(long)]
    open: bool,
    /// Embed screenshots in index.html (one file to share; bigger).
    #[arg(long)]
    single_file: bool,
    /// Treat pending (not yet implemented) steps as failures.
    #[arg(long)]
    fail_on_pending: bool,
    /// Run browser steps against the flows' screens and storyboards instead of
    /// the real app. Draft flows run too; http and exec steps are skipped.
    #[arg(long)]
    prototype: bool,
}

struct Colors {
    on: bool,
}

impl Colors {
    fn paint(&self, code: &str, s: &str) -> String {
        if self.on { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
    }
    fn green(&self, s: &str) -> String {
        self.paint("32", s)
    }
    fn red(&self, s: &str) -> String {
        self.paint("31", s)
    }
    fn yellow(&self, s: &str) -> String {
        self.paint("33", s)
    }
    fn purple(&self, s: &str) -> String {
        self.paint("35", s)
    }
    fn dim(&self, s: &str) -> String {
        self.paint("2", s)
    }
    fn bold(&self, s: &str) -> String {
        self.paint("1", s)
    }
}

fn colors() -> Colors {
    Colors { on: std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none() }
}

fn print_diags(diags: &[(String, Diagnostic)], c: &Colors) {
    for (file, d) in diags {
        let sev = match d.severity {
            Severity::Error => c.red("error"),
            Severity::Warning => c.yellow("warning"),
        };
        eprintln!("{file}:{}: {sev}: {}", d.line, d.message);
    }
}

fn fmt_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}m {:02}s", ms / 60_000, (ms % 60_000) / 1000)
    }
}

fn print_flow(f: &FlowResult, c: &Colors) {
    let t = &f.flow.title;
    let d = c.dim(&fmt_ms(f.duration_ms));
    match f.status {
        Status::Passed => eprintln!("  {} {t}  {d}", c.green("✓")),
        Status::Flaky => eprintln!("  {} {t}  {d}  {}", c.yellow("~"), c.yellow(&format!("flaky, passed on attempt {}", f.attempts.len()))),
        Status::Skipped | Status::NotRun => eprintln!("  {} {}  {}", c.dim("–"), c.dim(t), c.dim(&format!("skipped: {}", f.skip_reason.clone().unwrap_or_default()))),
        Status::Pending => {
            let n = f.attempts.last().map(|a| a.steps.iter().filter(|s| s.status == Status::Pending).count()).unwrap_or(0);
            let why = f.skip_reason.clone().unwrap_or_else(|| format!("{n} step(s) not implemented yet"));
            eprintln!("  {} {t}  {}", c.purple("◌"), c.purple(&why));
        }
        Status::Failed => {
            eprintln!("  {} {}  {d}", c.red("✗"), c.bold(t));
            if let Some(a) = f.attempts.last()
                && let Some(r) = a.steps.iter().find(|s| s.status == Status::Failed)
            {
                let step = f.flow.steps().find(|s| s.key == r.key);
                if let Some(s) = step {
                    eprintln!("      step {}  {}", s.number.map(|n| n.to_string()).unwrap_or_else(|| "cleanup".into()), s.intent);
                }
                if let Some(e) = &r.error {
                    for line in e.message.lines().take(4) {
                        eprintln!("              {}", c.red(line));
                    }
                }
            }
        }
    }
}

fn open_path(p: &std::path::Path) {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd).arg(p).spawn();
}

fn load_project(cli: &Cli) -> anyhow::Result<Project> {
    let start = cli.project.clone().unwrap_or_else(|| PathBuf::from("."));
    let project = if start.is_file() { Project::load(&start)? } else { Project::discover(&start)? };
    let env_file = cli.env_file.clone().or_else(|| Some(project.root.join(".enocone.env")).filter(|p| p.exists()));
    if let Some(f) = env_file {
        enocone_engine::config::load_env_file(&f)?;
    }
    Ok(project)
}

#[tokio::main]
async fn main() {
    let _ = tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_env("ENOCONE_LOG")).with_writer(std::io::stderr).try_init();
    let cli = Cli::parse();
    let code = match real_main(cli).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{} {e:#}", colors().red("error:"));
            3
        }
    };
    std::process::exit(code);
}

async fn real_main(cli: Cli) -> anyhow::Result<i32> {
    let c = colors();
    match &cli.cmd {
        Cmd::Init { name } => {
            init::init(&std::env::current_dir()?, name)?;
            Ok(0)
        }
        Cmd::InstallBrowsers { browsers, with_deps } => {
            let list = if browsers.is_empty() { vec!["chromium".to_string()] } else { browsers.clone() };
            enocone_driver_playwright::install_browsers(&list, *with_deps).await?;
            eprintln!("installed: {}", list.join(", "));
            Ok(0)
        }
        Cmd::Mcp => {
            // Load the project (and .enocone.env secrets) once, up front.
            let project = load_project(&cli)?;
            enocone_mcp::serve(project.root).await?;
            Ok(0)
        }
        Cmd::Report { results, out, open } => {
            let text = std::fs::read_to_string(results)?;
            let r: enocone_spec::RunResults = serde_json::from_str(&text)?;
            let dir = out.clone().unwrap_or_else(|| results.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")));
            let path = enocone_report::write_html(&r, &dir, &enocone_report::ReportOptions { inline_assets: false })?;
            eprintln!("report   {}", path.display());
            if *open {
                open_path(&path);
            }
            Ok(r.exit_code(false))
        }
        Cmd::Validate { paths, json } => {
            let project = load_project(&cli)?;
            let (flows, diags) = project.load_flows(paths)?;
            if *json {
                let list: Vec<_> = diags.iter().map(|(f, d)| serde_json::json!({ "file": f, "line": d.line, "severity": d.severity, "message": d.message })).collect();
                println!("{}", serde_json::to_string_pretty(&list)?);
            } else {
                print_diags(&diags, &c);
                let errors = diags.iter().filter(|(_, d)| d.severity == Severity::Error).count();
                let bad: std::collections::HashSet<&String> = diags.iter().filter(|(_, d)| d.severity == Severity::Error).map(|(f, _)| f).collect();
                let ok = flows.iter().filter(|f| !bad.iter().any(|b| std::path::Path::new(b.as_str()).ends_with(&f.path))).count();
                eprintln!("{} flows OK, {} errors, {} warnings", ok, errors, diags.len() - errors);
            }
            Ok(if diags.iter().any(|(_, d)| d.severity == Severity::Error) { 2 } else { 0 })
        }
        Cmd::List { paths } => {
            let project = load_project(&cli)?;
            let (flows, diags) = project.load_flows(paths)?;
            print_diags(&diags.into_iter().filter(|(_, d)| d.severity == Severity::Error).collect::<Vec<_>>(), &c);
            for f in flows {
                let total = f.steps().count();
                let todo = f.unimplemented_steps();
                let impl_state = if todo == 0 { c.green(&format!("{total}/{total} steps implemented")) } else { c.purple(&format!("{}/{total} steps implemented", total - todo)) };
                let q = if f.questions.is_empty() { String::new() } else { c.yellow(&format!(" · {} open question(s)", f.questions.len())) };
                println!("{:<12} {:<44} {}{}  {}", f.meta.stage.as_str(), f.id, impl_state, q, c.dim(&f.path));
            }
            Ok(0)
        }
        Cmd::Prototype { paths, port, open } => {
            let project = load_project(&cli)?;
            let (flows, diags) = project.load_flows(paths)?;
            print_diags(&diags, &c);
            let proto = enocone_mcp::prototype::Prototype::from_flows(&project, &flows);
            if proto.screens.is_empty() {
                eprintln!("no screens in these flows: add ```screen blocks (with `path:`) and a ```storyboard");
                return Ok(2);
            }
            let (addr, handle) = enocone_mcp::prototype::serve(proto.clone(), ([127, 0, 0, 1], *port).into()).await?;
            let url = format!("http://{addr}/");
            eprintln!("prototype {url}  ({} screens, {} links) · Ctrl-C to stop", proto.screens.len(), proto.transitions.len());
            for s in &proto.screens {
                eprintln!("  {:<16} http://{addr}{}", s.name, proto.url_of(&s.name, &Default::default()));
            }
            if *open {
                open_path(std::path::Path::new(&url));
            }
            let _ = tokio::signal::ctrl_c().await;
            handle.abort();
            Ok(0)
        }
        Cmd::RenderScreen { screen, out } => {
            let project = load_project(&cli)?;
            let png = app::render_screen(&project, &app::ScreenSource::Name(screen.clone())).await?;
            let out = out.clone().unwrap_or_else(|| {
                let name = screen.trim_end_matches(".html").replace(['/', '\\'], "-");
                project.root.join(".enocone/screens").join(format!("{name}.png"))
            });
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&out, png)?;
            eprintln!("screen   {}", out.display());
            Ok(0)
        }
        Cmd::Docs { paths, out, open } => {
            let project = load_project(&cli)?;
            let out = out.clone().unwrap_or_else(|| project.root.join("enocone-docs"));
            match app::docs(&project, paths, &out) {
                Ok((path, diags)) => {
                    print_diags(&diags, &c);
                    eprintln!("docs     {}", path.display());
                    if *open {
                        open_path(&path);
                    }
                    Ok(0)
                }
                Err(RunError::Spec(d)) => {
                    print_diags(&d, &c);
                    Ok(2)
                }
                Err(RunError::Infra(e)) => Err(e),
            }
        }
        Cmd::Run(a) => {
            let project = load_project(&cli)?;
            let out_dir = a.out.clone().unwrap_or_else(|| project.root.join("enocone-report"));
            let req = RunRequest {
                paths: a.paths.clone(),
                env: a.env.clone(),
                tags: a.tags.clone(),
                areas: a.areas.clone(),
                ids: a.ids.clone(),
                grep: a.grep.clone(),
                workers: a.workers,
                retries: a.retries,
                headed: a.headed,
                slow_mo_ms: a.slow_mo,
                browser: a.browser.clone(),
                channel: a.channel.clone(),
                out_dir: out_dir.clone(),
                inline_assets: a.single_file,
                prototype: a.prototype,
            };
            let env_name = project.pick_env(a.env.as_deref())?;
            let mode = if a.prototype { " · prototype" } else { "" };
            eprintln!("{}", c.bold(&format!("enocone {} · {} · env: {env_name}{mode}", env!("CARGO_PKG_VERSION"), project.config.project)));
            let cc = Colors { on: c.on };
            let events: enocone_engine::EventSink = Arc::new(move |e| match e {
                Event::FlowFinished(f) => print_flow(&f, &cc),
                Event::Retrying { id, attempt } => eprintln!("  {}", cc.yellow(&format!("↻ retrying {id} (attempt {attempt})"))),
                Event::Log(m) => eprintln!("  {}", cc.dim(&m)),
                Event::FlowStarted { .. } => {}
            });
            let out = match app::run(&project, &req, events).await {
                Ok(o) => o,
                Err(RunError::Spec(d)) => {
                    print_diags(&d, &c);
                    eprintln!("{}", c.red("fix the flow documents above, then run again"));
                    return Ok(2);
                }
                Err(RunError::Infra(e)) => {
                    eprintln!("{} {e:#}", c.red("error:"));
                    return Ok(3);
                }
            };
            let s = &out.results.summary;
            eprintln!();
            eprintln!(
                "{}   {}   {}   {}   {}     {}",
                c.bold(&format!("{} passed", s.passed)),
                if s.failed > 0 { c.red(&format!("{} failed", s.failed)) } else { format!("{} failed", s.failed) },
                format!("{} flaky", s.flaky),
                if s.pending > 0 { c.purple(&format!("{} pending", s.pending)) } else { format!("{} pending", s.pending) },
                format!("{} skipped", s.skipped),
                fmt_ms(out.results.run.duration_ms)
            );
            eprintln!();
            eprintln!("report   {}", out.report.display());
            eprintln!("{}", c.dim(&format!("junit    {}", out_dir.join("junit.xml").display())));
            eprintln!("{}", c.dim(&format!("json     {}", out_dir.join("results.json").display())));
            if let Ok(path) = std::env::var("GITHUB_STEP_SUMMARY") {
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(path) {
                    let _ = f.write_all(enocone_report::markdown_summary(&out.results, None).as_bytes());
                }
            }
            if a.open {
                open_path(&out.report);
            }
            Ok(out.results.exit_code(a.fail_on_pending))
        }
    }
}
