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

pub fn init(dir: &Path, name: &str) -> anyhow::Result<()> {
    if dir.join("enocone.toml").exists() {
        bail!("enocone.toml already exists here");
    }
    let write = |rel: &str, text: &str| -> anyhow::Result<()> {
        let p = dir.join(rel);
        if p.exists() {
            eprintln!("  kept     {rel} (already exists)");
            return Ok(());
        }
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p, text)?;
        eprintln!("  created  {rel}");
        Ok(())
    };
    write("enocone.toml", &CONFIG.replace("{{name}}", name))?;
    write("flows/getting-started/home-page.flow.md", FLOW)?;
    write("fragments/auth/sign-in.flow.md", SIGN_IN)?;
    write(".mcp.json", MCP)?;
    write(".enocone.env.example", ENV_EXAMPLE)?;
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
    eprintln!("\nNext: edit enocone.toml, then `enocone docs --open` or `enocone run --headed`.");
    Ok(())
}
