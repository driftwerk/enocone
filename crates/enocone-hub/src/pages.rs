//! Server-rendered pages in the enoc.one style. Everything interpolated goes
//! through `esc`; no page includes HTML that came from a publisher.

use crate::db::{Project, Role, User};

pub const MARK_ON_DARK: &str = r##"<svg width="30" height="30" viewBox="0 0 48 48" aria-hidden="true"><path d="M5 10 L21 24 L5 38 Z" fill="#8F7BFF"/><ellipse cx="5" cy="24" rx="3" ry="14" fill="#B9ACFF"/><path d="M43 10 L27 24 L43 38 Z" fill="#8F7BFF"/><ellipse cx="43" cy="24" rx="3" ry="14" fill="#B9ACFF"/><circle cx="24" cy="24" r="4.2" fill="none" stroke="#FFFFFF" stroke-width="2"/></svg>"##;
pub const FAVICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48"><path d="M5 10 L21 24 L5 38 Z" fill="#6B4EFF"/><path d="M43 10 L27 24 L43 38 Z" fill="#6B4EFF"/><circle cx="24" cy="24" r="5" fill="#6B4EFF"/></svg>"##;
const GITHUB: &str = r#"<svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M12 2a10 10 0 0 0-3.16 19.49c.5.09.68-.22.68-.48v-1.7c-2.78.6-3.37-1.34-3.37-1.34-.45-1.16-1.11-1.47-1.11-1.47-.91-.62.07-.6.07-.6 1 .07 1.53 1.03 1.53 1.03.9 1.52 2.34 1.08 2.91.83.09-.65.35-1.08.63-1.33-2.22-.25-4.55-1.11-4.55-4.94 0-1.09.39-1.98 1.03-2.68-.1-.25-.45-1.27.1-2.65 0 0 .84-.27 2.75 1.02a9.6 9.6 0 0 1 5 0c1.91-1.29 2.75-1.02 2.75-1.02.55 1.38.2 2.4.1 2.65.64.7 1.03 1.59 1.03 2.68 0 3.84-2.34 4.69-4.57 4.93.36.31.68.92.68 1.85v2.74c0 .27.18.58.69.48A10 10 0 0 0 12 2z"/></svg>"#;

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

pub fn doc(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{}</title>
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Fraunces:ital,opsz,wght@0,9..144,400;0,9..144,600;1,9..144,400&family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap">
<link rel="stylesheet" href="/assets/hub.css?v={}">
</head>
<body>
{body}
</body>
</html>
"#,
        esc(title),
        css_version()
    )
}

pub const HUB_CSS: &str = include_str!("../assets/hub.css");

/// Part of the stylesheet's hash, in its URL, so browsers fetch it again when it changes.
pub fn css_version() -> &'static str {
    use sha2::{Digest, Sha256};
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| Sha256::digest(HUB_CSS.as_bytes()).iter().take(6).map(|b| format!("{b:02x}")).collect())
}

/// The mark and the "eno·c·one" wordmark, the same as in the run report.
fn brand(site: &str) -> String {
    format!(r#"<a class="brand" href="/" aria-label="{} home">{MARK_ON_DARK}<span class="wordmark">eno<em>c</em>one</span></a>"#, esc(site))
}

/// The home page for signed-out visitors: what enocone is, with the sign-in card.
pub fn home(site: &str, origin: &str, github: bool, retention_days: u32, next: &str, error: Option<&str>) -> String {
    let gh_href = format!("/auth/github?next={}", esc(&urlenc(next)));
    let gh_block = if github {
        format!(
            r#"<a href="{gh_href}" class="h-btn" style="background:#0E1424;color:#fff">{GITHUB}Continue with GitHub</a>
        <div style="display:flex;align-items:center;gap:12px;color:#5F6678;font-size:13px"><span style="flex:1;height:1px;background:#E4E0D8"></span>or, without a GitHub account<span style="flex:1;height:1px;background:#E4E0D8"></span></div>"#
        )
    } else {
        String::new()
    };
    let gh_cta = if github { format!(r#"<a href="{gh_href}" class="h-btn" style="background:#FBFAF7;color:#0E1424">Continue with GitHub</a>"#) } else { String::new() };
    let err = error.map(|e| format!(r#"<div class="notice fail" role="alert">{}</div>"#, esc(e))).unwrap_or_default();
    let body = include_str!("../assets/home.html")
        .replace("{{SITE}}", &esc(site))
        .replace("{{ORIGIN}}", &esc(origin))
        .replace("{{GITHUB_BLOCK}}", &gh_block)
        .replace("{{GITHUB_CTA}}", &gh_cta)
        .replace("{{NEXT}}", &esc(next))
        .replace("{{ERROR}}", &err)
        .replace("{{RETENTION}}", &retention_days.to_string());
    doc(&format!("{site}: the spec your team reviews is the test that runs"), &body)
}

pub fn link_sent(site: &str, email: &str) -> String {
    doc(
        &format!("Check your inbox · {site}"),
        &format!(
            r#"<div class="cover gate"><main class="card">
  {brand}
  <div class="kicker">Check your inbox</div>
  <h1 class="serif" style="font-size:32px;line-height:1.1">If <em style="color:var(--iris)">{email}</em> is on a project, a sign-in link is on its way</h1>
  <p class="lede" style="font-size:17px">It works once and expires in 15 minutes.</p>
  <p class="small muted" style="margin:0">Nothing after a few minutes? Check spam, or ask a project owner whether they invited this address. <a href="/login">Use a different email</a></p>
</main></div>"#,
            brand = brand(site).replace("class=\"brand\"", "class=\"brand\" style=\"color:var(--ink)\""),
            email = esc(email)
        ),
    )
}

/// The page an email link opens: one button, so mail scanners that fetch links don't use them up.
pub fn confirm_link(site: &str, token: &str, email: &str) -> String {
    doc(
        &format!("Sign in · {site}"),
        &format!(
            r#"<div class="cover gate"><main class="card">
  <div class="kicker">Sign in</div>
  <h1 class="serif" style="font-size:30px;line-height:1.1">Continue as {email}</h1>
  <form method="post" action="/auth/email"><input type="hidden" name="token" value="{token}"><button class="btn iris wide" type="submit">Sign in to {site}</button></form>
</main></div>"#,
            email = esc(email),
            token = esc(token),
            site = esc(site)
        ),
    )
}

pub fn message(site: &str, title: &str, text: &str, link: Option<(&str, &str)>) -> String {
    let link = link.map(|(href, label)| format!(r#"<a class="btn ghost" href="{}">{}</a>"#, esc(href), esc(label))).unwrap_or_default();
    doc(
        &format!("{title} · {site}"),
        &format!(
            r#"<div class="cover gate"><main class="card"><div class="kicker">{site}</div><h1 class="serif" style="font-size:30px;line-height:1.1">{}</h1><p class="lede" style="font-size:17px">{}</p>{link}</main></div>"#,
            esc(title),
            esc(text),
            site = esc(site)
        ),
    )
}

pub struct Nav<'a> {
    pub site: &'a str,
    pub user: &'a User,
    pub projects: &'a [Project],
    pub current: Option<&'a Project>,
    pub section: &'a str,
    pub review_count: usize,
}

/// The app shell: the ink sidebar and a main column.
pub fn shell(nav: &Nav, title: &str, main: &str) -> String {
    let mut side = format!(r#"<nav class="side" aria-label="Projects and sections">{}<div class="h">Projects</div>"#, brand(nav.site));
    for p in nav.projects {
        let on = nav.current.is_some_and(|c| c.id == p.id);
        side.push_str(&format!(r#"<a class="proj{}" href="/{}"{}>{}</a>"#, if on { " on" } else { "" }, esc(&p.slug), if on { " aria-current=\"true\"" } else { "" }, esc(&p.name)));
    }
    if nav.projects.is_empty() {
        side.push_str(r#"<span style="padding:10px" class="small">No projects yet</span>"#);
    }
    if let Some(p) = nav.current {
        side.push_str(&format!(r#"<div class="h">{}</div>"#, esc(&p.name)));
        let count = if nav.review_count > 0 { format!(r#"<span class="count">{}</span>"#, nav.review_count) } else { String::new() };
        for (key, label, href, extra) in [
            ("flows", "Flows and runs", format!("/{}", p.slug), String::new()),
            ("review", "Review", format!("/{}/review", p.slug), count),
            ("members", "Members", format!("/{}/members", p.slug), String::new()),
            ("publish", "Publishing", format!("/{}/publish", p.slug), String::new()),
        ] {
            let on = nav.section == key;
            side.push_str(&format!(r#"<a href="{}" class="{}"{}>{}{extra}</a>"#, esc(&href), if on { "on" } else { "" }, if on { " aria-current=\"page\"" } else { "" }, label));
        }
    }
    side.push_str(&format!(r#"<div class="me"><span>Signed in as {}</span><form method="post" action="/logout"><button type="submit">Sign out</button></form></div></nav>"#, esc(&nav.user.label())));
    doc(title, &format!(r#"<div class="shell">{side}<div class="main">{main}</div></div>"#))
}

pub fn head(kicker: &str, title_html: &str, lede: &str, extra: &str) -> String {
    format!(
        r#"<header class="cover head"><div class="kicker">{}</div><h1>{title_html}</h1>{}{extra}</header>"#,
        esc(kicker),
        if lede.is_empty() { String::new() } else { format!(r#"<p class="lede">{}</p>"#, esc(lede)) }
    )
}

pub fn role_pill(r: Role) -> String {
    let cls = match r {
        Role::Owner => "ink",
        Role::Member => "iris",
        Role::Reviewer => "amber",
    };
    format!(r#"<span class="pill {cls}">{}</span>"#, r.title())
}

pub fn urlenc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}
