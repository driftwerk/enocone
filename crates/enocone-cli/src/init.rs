//! `enocone init`: a starting project.

use std::path::Path;

use anyhow::bail;

const CONFIG: &str = r#"project = "{{name}}"
default_env = "local"

[run]
workers = 4
retries = 0
timeout = "10s"
screenshots = "every-step"   # every-step | on-failure | never

[env.local]
API = "http://localhost:8080/api"
WEB = "http://localhost:3000"

[env.staging]
API = "https://api.staging.example.com/api"
WEB = "https://staging.example.com"

# Sign in through the web UI once per run; browser steps use `as: user`,
# http steps use `auth: user` (bearer token read from the signed-in page).
[auth.user]
type = "browser"
login = "auth/sign-in"
params = { username = "{{secret.APP_USER}}", password = "{{secret.APP_PASSWORD}}" }
# token = { url = "{{env.WEB}}/api/auth/session", path = "$.accessToken" }

# [auth.api-key]
# type = "header"
# header = "X-API-Key"
# value = "{{secret.APP_API_KEY}}"
"#;

const FLOW: &str = r#"---
area: Getting started
tags: [smoke]
stage: draft
---

# A visitor can reach the home page

The simplest promise the product makes: it is up. Replace this flow with a
real business flow, or ask your agent to design some (`enocone mcp`).

## When

1. **A visitor opens the home page**
   ```browser
   - goto: /
   ```

## Then

2. **The page has a main heading**
   ```expect
   role: heading
   ```

## Open questions

- Which heading text should we check for?
"#;

const SIGN_IN: &str = r#"# Sign in

Shared login used by the `user` auth profile.

## When

1. **Sign in with a username and password**
   ```browser
   - goto: /
   - fill: { label: "Username or email", value: "{{username}}" }
   - fill: { label: Password, value: "{{password}}" }
   - click: { role: button, name: Sign In }
   ```

## Then

2. **The app is shown signed in**
   ```expect
   url: "{{env.WEB}}"
   ```
"#;

const MCP: &str = r#"{
  "mcpServers": {
    "enocone": { "command": "enocone", "args": ["mcp"] }
  }
}
"#;

const ENV_EXAMPLE: &str = "# Copy to .enocone.env (git-ignored) for local secrets.\nAPP_USER=\nAPP_PASSWORD=\n";

/// The team site to connect to, from `enocone init --name SLUG --hub URL`.
pub struct Hub<'a> {
    pub url: &'a str,
    pub project: &'a str,
}

const WORKFLOW: &str = r#"# Runs the flows on every push and pull request, and publishes each run to
# {{url}}/{{project}}. GitHub vouches for the workflow (id-token: write), so
# no secret is stored. Created by `enocone init`.
name: enocone
on:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read
  id-token: write

jobs:
  flows:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: driftwerk/enocone@v0
        with:
          project: {{dir}}
          publish: true
"#;

fn hub_section(h: &Hub) -> String {
    format!("\n# The team site: `enocone publish` and CI send runs here.\n[hub]\nurl = \"{}\"\nproject = \"{}\"\n", h.url, h.project)
}

/// The repository root above `dir` (the folder holding `.git`), if any.
fn git_root(dir: &Path) -> Option<std::path::PathBuf> {
    dir.ancestors().find(|d| d.join(".git").exists()).map(Path::to_path_buf)
}

pub fn init(dir: &Path, name: &str, hub: Option<Hub>) -> anyhow::Result<()> {
    if let Some(h) = &hub
        && !(h.url.starts_with("https://") || h.url.starts_with("http://"))
    {
        bail!("--hub must be the site's address, e.g. https://enoc.one");
    }
    let write = |p: &Path, shown: &str, text: &str| -> anyhow::Result<()> {
        if p.exists() {
            eprintln!("  kept     {shown} (already exists)");
            return Ok(());
        }
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(p, text)?;
        eprintln!("  created  {shown}");
        Ok(())
    };
    let at = |rel: &str| dir.join(rel);
    let toml = at("enocone.toml");
    let mut config = CONFIG.replace("{{name}}", name);
    if let Some(h) = &hub {
        config.push_str(&hub_section(h));
    }
    if toml.exists() {
        let text = std::fs::read_to_string(&toml)?;
        match &hub {
            Some(h) if !text.lines().any(|l| l.trim() == "[hub]") => {
                let mut text = text;
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&hub_section(h));
                std::fs::write(&toml, text)?;
                eprintln!("  updated  enocone.toml (added [hub])");
            }
            _ => eprintln!("  kept     enocone.toml (already exists)"),
        }
    } else {
        write(&toml, "enocone.toml", &config)?;
    }
    write(&at("flows/getting-started/home-page.flow.md"), "flows/getting-started/home-page.flow.md", FLOW)?;
    write(&at("fragments/auth/sign-in.flow.md"), "fragments/auth/sign-in.flow.md", SIGN_IN)?;
    write(&at(".mcp.json"), ".mcp.json", MCP)?;
    write(&at(".enocone.env.example"), ".enocone.env.example", ENV_EXAMPLE)?;
    let mut repo_slug = None;
    if let Some(h) = &hub {
        let root = git_root(dir).unwrap_or_else(|| dir.to_path_buf());
        let rel = dir.strip_prefix(&root).ok().map(|p| p.to_string_lossy().replace('\\', "/")).filter(|p| !p.is_empty()).unwrap_or_else(|| ".".into());
        let wf = root.join(".github/workflows/enocone.yml");
        let shown = if rel == "." { ".github/workflows/enocone.yml".to_string() } else { wf.strip_prefix(dir).map(|p| p.display().to_string()).unwrap_or_else(|_| wf.display().to_string()) };
        write(&wf, &shown, &WORKFLOW.replace("{{url}}", h.url).replace("{{project}}", h.project).replace("{{dir}}", &rel))?;
        repo_slug = github_repo(&root);
    }
    let gi = dir.join(".gitignore");
    let mut text = std::fs::read_to_string(&gi).unwrap_or_default();
    for line in [".enocone.env", "enocone-report/", "enocone-docs/", ".enocone/"] {
        if !text.lines().any(|l| l.trim() == line) {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(line);
            text.push('\n');
        }
    }
    std::fs::write(gi, text)?;
    match &hub {
        None => eprintln!("\nNext: edit enocone.toml, then `enocone docs --open` or `enocone run --headed`."),
        Some(h) => {
            let repo = repo_slug.as_deref().unwrap_or("owner/name");
            eprintln!(
                "\nNext:
  1. Sign in to the site:            enocone login
  2. On {url}, create the project `{project}` and link it to the GitHub repository {repo},
     so CI can publish without a secret.
  3. Edit enocone.toml (environments, sign-in), then commit and push.
  4. Open your AI tool in this folder: .mcp.json starts enocone's MCP server.
     Claude Code, in every project:  claude mcp add enocone --scope user -- enocone mcp
     Then ask it to design flows for a feature, or to describe what the app does today.",
                url = h.url,
                project = h.project
            );
        }
    }
    Ok(())
}

/// `owner/name` of the repository's GitHub remote, if it has one.
fn github_repo(root: &Path) -> Option<String> {
    let out = std::process::Command::new("git").arg("-C").arg(root).args(["remote", "get-url", "origin"]).output().ok()?;
    let url = String::from_utf8(out.stdout).ok()?;
    let url = url.trim().trim_end_matches(".git");
    let rest = url.strip_prefix("git@github.com:").or_else(|| url.strip_prefix("https://github.com/")).or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
    (rest.split('/').count() == 2).then(|| rest.to_string())
}
