//! Sign-in and invitation emails through Postmark. Without a token the
//! message is written to the log instead, so a fresh server can still be
//! signed into (see also `enocone serve --sign-in-link`).

use serde_json::json;

#[derive(Clone)]
pub struct Mailer {
    pub postmark_url: String,
    pub token: Option<String>,
    pub from: String,
    http: reqwest::Client,
}

impl Mailer {
    pub fn new(postmark_url: String, token: Option<String>, from: String) -> Mailer {
        Mailer { postmark_url, token, from, http: reqwest::Client::new() }
    }

    pub async fn send(&self, to: &str, subject: &str, text: &str, html: &str) -> anyhow::Result<()> {
        let Some(token) = &self.token else {
            tracing::warn!("no ENOCONE_POSTMARK_TOKEN: email to {to} not sent. {subject}\n{text}");
            eprintln!("[enocone serve] email to {to} (no mail provider configured)\n  {subject}\n{text}");
            return Ok(());
        };
        let body = json!({ "From": self.from, "To": to, "Subject": subject, "TextBody": text, "HtmlBody": html, "MessageStream": "outbound" });
        let res = self.http.post(format!("{}/email", self.postmark_url.trim_end_matches('/'))).header("X-Postmark-Server-Token", token).header("Accept", "application/json").json(&body).send().await?;
        if !res.status().is_success() {
            anyhow::bail!("Postmark refused the email to {to}: {} {}", res.status(), res.text().await.unwrap_or_default());
        }
        Ok(())
    }
}
