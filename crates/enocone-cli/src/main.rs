#![allow(clippy::format_in_format_args)] // coloured summary line reads better inline
//! `enocone`: business flow documents that run as end-to-end tests.

mod hub;
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
    /// Run the team site where flow documents, runs and review comments are shared.
    Serve(ServeArgs),
    /// Sign in to a team site (enoc.one by default) so `enocone publish` can send runs.
    Login {
        /// The site (default: [hub] url in enocone.toml, or https://enoc.one).
        #[arg(long)]
        url: Option<String>,
        /// Read a publish token from stdin instead of opening a browser.
        #[arg(long)]
        with_token: bool,
    },
    /// Forget the token `enocone login` saved on this computer.
    Logout {
        #[arg(long)]
        url: Option<String>,
    },
    /// Send the latest run (results and screenshots) to the team site.
    Publish {
        /// results.json to send (default: enocone-report/results.json in the project).
        results: Option<PathBuf>,
        /// The site (default: [hub] url in enocone.toml, or https://enoc.one).
        #[arg(long)]
        url: Option<String>,
        /// The project on the site (default: [hub] project in enocone.toml).
        #[arg(long = "to")]
        to: Option<String>,
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

#[derive(clap::Args)]
struct ServeArgs {
    /// The address people use, e.g. https://enoc.one (links in emails and secure cookies follow it).
    #[arg(long, env = "ENOCONE_PUBLIC_URL", default_value = "http://127.0.0.1:8080")]
    public_url: String,
    /// Where to listen; put Caddy or another proxy in front for HTTPS.
    #[arg(long, env = "ENOCONE_LISTEN", default_value = "127.0.0.1:8080")]
    listen: std::net::SocketAddr,
    /// Data folder: the SQLite database and published runs.
    #[arg(long, env = "ENOCONE_DATA", default_value = "enocone-data")]
    data: PathBuf,
    /// The site owner: may always sign in and creates projects.
    #[arg(long, env = "ENOCONE_OWNER_EMAIL")]
    owner_email: Option<String>,
    #[arg(long, env = "ENOCONE_GITHUB_CLIENT_ID", hide_env_values = true)]
    github_client_id: Option<String>,
    #[arg(long, env = "ENOCONE_GITHUB_CLIENT_SECRET", hide_env_values = true)]
    github_client_secret: Option<String>,
    /// Postmark server token for sign-in emails. Without one, emails are printed to the log.
    #[arg(long, env = "ENOCONE_POSTMARK_TOKEN", hide_env_values = true)]
    postmark_token: Option<String>,
    #[arg(long, env = "ENOCONE_POSTMARK_URL", default_value = "https://api.postmarkapp.com", hide = true)]
    postmark_url: String,
    /// The From address of sign-in emails (default: noreply@ the site's host).
    #[arg(long, env = "ENOCONE_MAIL_FROM")]
    mail_from: Option<String>,
    /// Who signs CI tokens (default: GitHub Actions).
    #[arg(long, env = "ENOCONE_OIDC_ISSUER", default_value = enocone_hub::GITHUB_ISSUER, hide = true)]
    oidc_issuer: String,
    /// Days runs are kept (flow documents and comments are kept).
    #[arg(long, env = "ENOCONE_RUN_RETENTION_DAYS", default_value_t = 30)]
    run_retention_days: u32,
    /// Print a one-time sign-in link for this address and exit (first sign-in, before email is set up).
    #[arg(long, value_name = "EMAIL")]
    sign_in_link: Option<String>,
    /// Print a new publish token for this address and exit (e.g. for a CI secret).
    #[arg(long, value_name = "EMAIL")]
    new_token: Option<String>,
    /// Delete expired runs now and exit.
    #[arg(long)]
    sweep: bool,
    /// Testing: sweep as if the clock were this many days ahead.
    #[arg(long, hide = true, default_value_t = 0, requires = "sweep")]
    clock_offset_days: i64,
}

/// The team site: --url, else [hub] url in enocone.toml, else enoc.one.
fn hub_url(cli: &Cli, flag: Option<&str>) -> String {
    if let Some(u) = flag {
        return hub::normalise_url(u);
    }
    let from_project = load_project(cli).ok().and_then(|p| p.config.hub).map(|h| h.url);
    hub::normalise_url(from_project.as_deref().unwrap_or(hub::DEFAULT_HUB))
}

fn non_empty(v: &Option<String>) -> Option<String> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

async fn serve(a: &ServeArgs) -> anyhow::Result<i32> {
    let host = a.public_url.split("://").nth(1).unwrap_or(&a.public_url).split(['/', ':']).next().unwrap_or("localhost").to_string();
    let cfg = enocone_hub::Config {
        public_url: a.public_url.trim_end_matches('/').to_string(),
        listen: a.listen,
        data: a.data.clone(),
        owner_email: non_empty(&a.owner_email).map(|e| e.to_lowercase()),
        github_client_id: non_empty(&a.github_client_id),
        github_client_secret: non_empty(&a.github_client_secret),
        postmark_token: non_empty(&a.postmark_token),
        postmark_url: a.postmark_url.clone(),
        mail_from: non_empty(&a.mail_from).unwrap_or_else(|| format!("noreply@{host}")),
        run_retention_days: a.run_retention_days,
        oidc_issuer: a.oidc_issuer.clone(),
    };
    if let Some(email) = &a.sign_in_link {
        println!("{}", enocone_hub::App::new(cfg)?.sign_in_link(email)?);
        return Ok(0);
    }
    if let Some(email) = &a.new_token {
        println!("{}", enocone_hub::App::new(cfg)?.create_token(email, "created by enocone serve --new-token")?);
        return Ok(0);
    }
    if a.sweep {
        let n = enocone_hub::App::new(cfg)?.sweep_now(a.clock_offset_days)?;
        println!("deleted {n} expired run{}", if n == 1 { "" } else { "s" });
        return Ok(0);
    }
    enocone_hub::serve(cfg).await?;
    Ok(0)
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

fn open_url(u: &str) {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd).arg(u).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
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
        Cmd::Serve(a) => serve(a).await,
        Cmd::Login { url, with_token } => {
            hub::login(&hub_url(&cli, url.as_deref()), *with_token).await?;
            Ok(0)
        }
        Cmd::Logout { url } => {
            hub::logout(&hub_url(&cli, url.as_deref()))?;
            Ok(0)
        }
        Cmd::Publish { results, url, to } => {
            let project = load_project(&cli).ok();
            let hub_cfg = project.as_ref().and_then(|p| p.config.hub.clone());
            let url = hub_url(&cli, url.as_deref());
            let Some(slug) = to.clone().or_else(|| hub_cfg.map(|h| h.project)) else {
                anyhow::bail!("which project? Add [hub] project = \"…\" to enocone.toml, or pass --to");
            };
            let path = match (results, &project) {
                (Some(r), _) => r.clone(),
                (None, Some(p)) => p.root.join("enocone-report").join("results.json"),
                (None, None) => PathBuf::from("enocone-report/results.json"),
            };
            println!("{}", hub::publish(&url, &slug, &path).await?);
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
