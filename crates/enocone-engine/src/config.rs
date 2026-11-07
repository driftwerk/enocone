//! `enocone.toml`: environments, auth profiles and run defaults.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use enocone_spec::design::Screen;
use enocone_spec::{Diagnostic, Flow, Severity, parse_flow};
use serde::Deserialize;

pub const CONFIG_FILE: &str = "enocone.toml";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub project: String,
    #[serde(default = "default_flows")]
    pub flows: String,
    #[serde(default = "default_fragments")]
    pub fragments: String,
    /// Mockup screens: `<screens>/<name>.html`.
    #[serde(default = "default_screens")]
    pub screens: String,
    /// Look and feel for mockups: `theme.css` and/or `tokens.json` (W3C design tokens).
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub default_env: Option<String>,
    #[serde(default)]
    pub run: RunConfig,
    #[serde(default)]
    pub env: BTreeMap<String, BTreeMap<String, toml::Value>>,
    #[serde(default)]
    pub auth: BTreeMap<String, AuthProfile>,
    /// The sandbox mailbox email blocks read.
    #[serde(default)]
    pub email: Option<EmailConfig>,
}

/// `[email]`: where test emails land. `provider` is postmark, mailpit or mailhog.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmailConfig {
    pub provider: String,
    /// API address (default: Postmark's API, or http://127.0.0.1:8025 for Mailpit/MailHog).
    #[serde(default)]
    pub url: Option<String>,
    /// Postmark: a sandbox server's API token. Mailpit/MailHog: `user:password` or a bearer token.
    #[serde(default)]
    pub token: Option<String>,
}

fn default_flows() -> String {
    "flows".into()
}
fn default_fragments() -> String {
    "fragments".into()
}
fn default_screens() -> String {
    "screens".into()
}
fn default_theme() -> String {
    "theme".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    #[serde(default = "default_workers")]
    pub workers: usize,
    #[serde(default)]
    pub retries: u32,
    /// Default wait for elements and assertions.
    #[serde(default = "default_timeout")]
    pub timeout: String,
    #[serde(default = "default_nav_timeout")]
    pub navigation_timeout: String,
    #[serde(default)]
    pub screenshots: Screenshots,
    #[serde(default = "default_browser")]
    pub browser: String,
    #[serde(default = "yes")]
    pub headless: bool,
    /// Accept self-signed certificates (local stacks).
    #[serde(default)]
    pub insecure: bool,
    /// Allow `exec` blocks to run commands. Off by default: a flow (or an AI
    /// agent dry-running one) can otherwise run anything on this machine.
    #[serde(default)]
    pub allow_exec: bool,
}

impl Default for RunConfig {
    fn default() -> Self {
        RunConfig {
            workers: default_workers(),
            retries: 0,
            timeout: default_timeout(),
            navigation_timeout: default_nav_timeout(),
            screenshots: Screenshots::default(),
            browser: default_browser(),
            headless: true,
            insecure: false,
            allow_exec: false,
        }
    }
}

fn default_workers() -> usize {
    4
}
fn default_timeout() -> String {
    "10s".into()
}
fn default_nav_timeout() -> String {
    "30s".into()
}
fn default_browser() -> String {
    "chromium".into()
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Screenshots {
    /// After every browser step (default: the report shows what people saw).
    #[default]
    EveryStep,
    OnFailure,
    Never,
}

/// How a named profile authenticates. Values may use `{{env.*}}` / `{{secret.*}}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AuthProfile {
    /// A fixed header, e.g. an API key.
    Header { header: String, value: String },
    /// `Authorization: Bearer <token>`.
    Bearer { token: String },
    /// OAuth2 token endpoint (password or client_credentials grant).
    Oauth2 {
        token_url: String,
        #[serde(default = "default_grant")]
        grant: String,
        client_id: String,
        #[serde(default)]
        client_secret: Option<String>,
        #[serde(default)]
        username: Option<String>,
        #[serde(default)]
        password: Option<String>,
        #[serde(default)]
        scope: Option<String>,
    },
    /// Sign in through the web UI by running a fragment, then reuse the
    /// session for browser steps (`as: <profile>`) and, if `token` is set,
    /// read a bearer token for http steps from the signed-in page.
    Browser {
        /// Fragment under `fragments/` that performs the login.
        login: String,
        #[serde(default)]
        params: BTreeMap<String, String>,
        #[serde(default)]
        token: Option<BrowserToken>,
    },
}

fn default_grant() -> String {
    "password".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserToken {
    /// URL fetched from inside the signed-in page (same origin), e.g. next-auth's `/api/auth/session`.
    pub url: String,
    /// JSONPath into the response, e.g. `$.accessToken`.
    pub path: String,
    /// Re-read the token when it is this old (e.g. "60s"), using the saved
    /// browser session — for systems whose access tokens last only minutes.
    #[serde(default)]
    pub refresh: Option<String>,
}

/// Diagnostics tagged with the file they belong to.
pub type FileDiagnostics = Vec<(String, Diagnostic)>;

/// A loaded project: config plus its root directory.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub config: Config,
}

impl Project {
    /// Find `enocone.toml` in `start` or a parent directory.
    pub fn discover(start: &Path) -> anyhow::Result<Project> {
        let start = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
        let mut dir = Some(start.as_path());
        while let Some(d) = dir {
            let candidate = d.join(CONFIG_FILE);
            if candidate.exists() {
                return Project::load(&candidate);
            }
            dir = d.parent();
        }
        bail!("no {CONFIG_FILE} found in {} or its parents (run `enocone init`)", start.display())
    }

    pub fn load(file: &Path) -> anyhow::Result<Project> {
        let text = std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
        let config: Config = toml::from_str(&text).with_context(|| format!("parsing {}", file.display()))?;
        for (name, p) in &config.auth {
            if let AuthProfile::Browser { token: Some(t), .. } = p
                && let Some(r) = &t.refresh
            {
                enocone_spec::blocks::parse_duration_str(r).map_err(|e| anyhow::anyhow!("{}: auth.{name}.token.refresh: {e}", file.display()))?;
            }
        }
        if let Some(e) = &config.email {
            crate::email::Provider::parse(&e.provider).map_err(|m| anyhow::anyhow!("{}: [email] {m}", file.display()))?;
        }
        let root = file.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        Ok(Project { root, config })
    }

    pub fn flows_dir(&self) -> PathBuf {
        self.root.join(&self.config.flows)
    }
    pub fn fragments_dir(&self) -> PathBuf {
        self.root.join(&self.config.fragments)
    }

    pub fn env_names(&self) -> Vec<String> {
        self.config.env.keys().cloned().collect()
    }

    pub fn pick_env(&self, requested: Option<&str>) -> anyhow::Result<String> {
        let name = requested.map(String::from).or_else(|| self.config.default_env.clone()).or_else(|| self.config.env.keys().next().cloned()).unwrap_or_else(|| "default".into());
        if !self.config.env.is_empty() && !self.config.env.contains_key(&name) {
            bail!("unknown env `{name}` (configured: {})", self.env_names().join(", "));
        }
        Ok(name)
    }

    /// Variables of an environment as JSON (strings, numbers, booleans).
    pub fn env_vars(&self, env: &str) -> serde_json::Map<String, serde_json::Value> {
        let mut out = serde_json::Map::new();
        if let Some(vars) = self.config.env.get(env) {
            for (k, v) in vars {
                out.insert(k.clone(), toml_to_json(v));
            }
        }
        // ENOCONE_VAR_<NAME> overrides, handy in CI.
        for (k, v) in std::env::vars() {
            if let Some(name) = k.strip_prefix("ENOCONE_VAR_") {
                out.insert(name.to_string(), serde_json::Value::String(v));
            }
        }
        out
    }

    /// Path relative to the project root, forward slashes.
    pub fn rel(&self, p: &Path) -> String {
        let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let root = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        p.strip_prefix(&root).unwrap_or(&p).to_string_lossy().replace('\\', "/")
    }

    /// Every `.flow.md` under the given paths (files or directories; default: the flows dir).
    pub fn flow_files(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
        let roots: Vec<PathBuf> = if paths.is_empty() { vec![self.flows_dir()] } else { paths.to_vec() };
        let mut out = Vec::new();
        for r in roots {
            if r.is_file() {
                out.push(r);
                continue;
            }
            if !r.exists() {
                bail!("{} does not exist", r.display());
            }
            for e in walkdir::WalkDir::new(&r).sort_by_file_name() {
                let e = e?;
                if e.file_type().is_file() && e.file_name().to_string_lossy().ends_with(".flow.md") {
                    out.push(e.into_path());
                }
            }
        }
        Ok(out)
    }

    /// Parse flows; returns the flows and every diagnostic, tagged with the file.
    pub fn load_flows(&self, paths: &[PathBuf]) -> anyhow::Result<(Vec<Flow>, FileDiagnostics)> {
        let mut flows = Vec::new();
        let mut diags = Vec::new();
        let mut ids: BTreeMap<String, String> = BTreeMap::new();
        for file in self.flow_files(paths)? {
            let rel = self.rel(&file);
            let text = std::fs::read_to_string(&file).with_context(|| format!("reading {rel}"))?;
            let parsed = parse_flow(&rel, &text);
            if let Some(other) = ids.insert(parsed.flow.id.clone(), rel.clone()) {
                diags.push((rel.clone(), Diagnostic { line: 1, severity: Severity::Error, message: format!("duplicate flow id `{}` (also in {other})", parsed.flow.id) }));
            }
            for d in &parsed.diagnostics {
                diags.push((rel.clone(), d.clone()));
            }
            if !parsed.has_errors() {
                let mut flow = parsed.flow;
                for d in self.attach_screens(&mut flow) {
                    diags.push((rel.clone(), d));
                }
                flows.push(flow);
            }
        }
        Ok((flows, diags))
    }

    /// Fill in each named screen's HTML from `screens/<name>.html`, and check
    /// that storyboards only mention screens that exist.
    pub fn attach_screens(&self, flow: &mut Flow) -> Vec<Diagnostic> {
        let mut diags = vec![];
        let dir = self.root.join(&self.config.screens);
        let lines: Vec<(Option<String>, usize)> = flow
            .steps()
            .flat_map(|st| st.designs.iter().map(|d| (d.screen.as_ref().and_then(|s| s.name.clone()), d.line)))
            .chain(flow.designs.iter().map(|d| (d.screen.as_ref().and_then(|s| s.name.clone()), d.line)))
            .collect();
        for s in flow.screens_mut() {
            if s.html.is_some() {
                continue;
            }
            let Some(name) = s.name.clone() else { continue };
            let file = dir.join(format!("{name}.html"));
            match std::fs::read_to_string(&file) {
                Ok(html) => s.html = Some(html),
                Err(_) => {
                    let line = lines.iter().find(|(n, _)| n.as_deref() == Some(name.as_str())).map(|(_, l)| *l).unwrap_or(1);
                    diags.push(Diagnostic { line, severity: Severity::Error, message: format!("screen `{name}` not found: expected {}/{name}.html", self.config.screens) });
                }
            }
        }
        let known: Vec<Screen> = flow.screens().map(|(_, s)| s.clone()).collect();
        for d in &mut flow.designs {
            let mut names: Vec<String> = vec![];
            for t in &d.transitions {
                for name in [&t.from, &t.to] {
                    if !names.contains(name) {
                        names.push(name.clone());
                    }
                }
            }
            for name in names {
                if let Some(s) = known.iter().find(|s| s.name.as_deref() == Some(name.as_str()) && s.html.is_some()) {
                    d.screens.push(s.clone());
                } else if let Ok(html) = std::fs::read_to_string(dir.join(format!("{name}.html"))) {
                    d.screens.push(Screen { name: Some(name), html: Some(html), ..Default::default() });
                } else if !known.iter().any(|s| s.name.as_deref() == Some(name.as_str())) {
                    diags.push(Diagnostic { line: d.line, severity: Severity::Error, message: format!("storyboard mentions screen `{name}`, but there is no {}/{name}.html", self.config.screens) });
                }
            }
        }
        diags
    }

    /// The project's theme as CSS: `theme/theme.css`, plus variables from `theme/tokens.json`.
    pub fn theme_css(&self) -> Option<String> {
        let dir = self.root.join(&self.config.theme);
        let mut css = String::new();
        if let Ok(t) = std::fs::read_to_string(dir.join("tokens.json"))
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&t)
        {
            let mut vars = vec![];
            tokens_to_vars(&v, &mut vec![], &mut vars);
            if !vars.is_empty() {
                css.push_str(":root {\n");
                for (k, v) in vars {
                    css.push_str(&format!("  --{k}: {v};\n"));
                }
                css.push_str("}\n");
            }
        }
        if let Ok(t) = std::fs::read_to_string(dir.join("theme.css")) {
            css.push_str(&t);
        }
        (!css.trim().is_empty()).then_some(css)
    }

    pub fn load_fragment(&self, name: &str) -> anyhow::Result<Flow> {
        let name = name.trim_end_matches(".flow.md");
        let file = self.fragments_dir().join(format!("{name}.flow.md"));
        let text = std::fs::read_to_string(&file).with_context(|| format!("fragment `{name}` not found at {}", self.rel(&file)))?;
        let parsed = parse_flow(&self.rel(&file), &text);
        let errors: Vec<String> = parsed.diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| d.to_string()).collect();
        if !errors.is_empty() {
            bail!("fragment `{name}` has errors: {}", errors.join("; "));
        }
        Ok(parsed.flow)
    }
}

fn toml_to_json(v: &toml::Value) -> serde_json::Value {
    match v {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::Value::from(*i),
        toml::Value::Float(f) => serde_json::Value::from(*f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Datetime(d) => serde_json::Value::String(d.to_string()),
        toml::Value::Array(a) => serde_json::Value::Array(a.iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => serde_json::Value::Object(t.iter().map(|(k, v)| (k.clone(), toml_to_json(v))).collect()),
    }
}

/// Load `KEY=value` lines from a dotenv-style file into the process
/// environment (existing variables win). Used for local secrets.
pub fn load_env_file(path: &Path) -> anyhow::Result<usize> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut n = 0;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else { continue };
        let k = k.trim();
        let v = v.trim().trim_matches('"').trim_matches('\'');
        if std::env::var_os(k).is_none() {
            // SAFETY: called at startup before worker threads read the environment.
            unsafe { std::env::set_var(k, v) };
            n += 1;
        }
    }
    Ok(n)
}

/// Flatten W3C design tokens (`{"color": {"primary": {"$value": "#1D4BE5"}}}`)
/// into CSS variables (`--color-primary`). `{color.blue}` references become `var(--color-blue)`.
fn tokens_to_vars(v: &serde_json::Value, path: &mut Vec<String>, out: &mut Vec<(String, String)>) {
    use serde_json::Value;
    let Some(m) = v.as_object() else { return };
    if let Some(val) = m.get("$value").or_else(|| m.get("value").filter(|_| m.contains_key("type") || m.contains_key("$type"))) {
        let css = match val {
            Value::String(s) if s.starts_with('{') && s.ends_with('}') => format!("var(--{})", s[1..s.len() - 1].replace('.', "-")),
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Object(o) if o.contains_key("value") && o.contains_key("unit") => format!("{}{}", o["value"], o["unit"].as_str().unwrap_or("")),
            Value::Array(a) => a.iter().filter_map(|x| x.as_str()).map(|s| if s.contains(' ') { format!("\"{s}\"") } else { s.to_string() }).collect::<Vec<_>>().join(", "),
            _ => return,
        };
        out.push((path.join("-").to_lowercase().replace(' ', "-"), css));
        return;
    }
    for (k, child) in m {
        if k.starts_with('$') {
            continue;
        }
        path.push(k.clone());
        tokens_to_vars(child, path, out);
        path.pop();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn design_tokens_become_css_variables() {
        let v: serde_json::Value = serde_json::from_str(r##"{"color":{"$type":"color","primary":{"$value":"#1D4BE5"},"accent":{"$value":"{color.primary}"}},"space":{"md":{"$value":{"value":12,"unit":"px"}}},"font":{"body":{"$value":["Inter","Helvetica Neue"]}}}"##).unwrap();
        let mut out = vec![];
        super::tokens_to_vars(&v, &mut vec![], &mut out);
        assert!(out.contains(&("color-primary".into(), "#1D4BE5".into())), "{out:?}");
        assert!(out.contains(&("color-accent".into(), "var(--color-primary)".into())), "{out:?}");
        assert!(out.contains(&("space-md".into(), "12px".into())), "{out:?}");
        assert!(out.contains(&("font-body".into(), "Inter, \"Helvetica Neue\"".into())), "{out:?}");
    }
}
