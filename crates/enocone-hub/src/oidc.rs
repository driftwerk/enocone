//! GitHub Actions OIDC: CI publishes without a stored secret. GitHub signs a
//! short-lived token naming the repository; the site checks the signature
//! against GitHub's published keys, the issuer, the audience (this site's URL)
//! and expiry, then accepts it only for a project linked to that repository.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;

pub const GITHUB_ISSUER: &str = "https://token.actions.githubusercontent.com";

#[derive(Debug, Clone, Deserialize)]
pub struct Claims {
    pub repository: String,
    #[serde(default)]
    pub event_name: String,
    #[serde(default)]
    pub actor: String,
}

pub struct Verifier {
    issuer: String,
    http: reqwest::Client,
    keys: Mutex<Option<(Instant, JwkSet)>>,
}

impl Verifier {
    pub fn new(issuer: String) -> Verifier {
        Verifier { issuer: issuer.trim_end_matches('/').to_string(), http: reqwest::Client::builder().timeout(Duration::from_secs(10)).build().expect("http client"), keys: Mutex::new(None) }
    }

    async fn jwks(&self, refresh: bool) -> anyhow::Result<JwkSet> {
        if !refresh
            && let Some((at, set)) = self.keys.lock().unwrap().as_ref()
            && at.elapsed() < Duration::from_secs(600)
        {
            return Ok(set.clone());
        }
        #[derive(Deserialize)]
        struct Discovery {
            jwks_uri: String,
        }
        let d: Discovery = self.http.get(format!("{}/.well-known/openid-configuration", self.issuer)).send().await?.error_for_status()?.json().await?;
        let set: JwkSet = self.http.get(&d.jwks_uri).send().await?.error_for_status()?.json().await?;
        *self.keys.lock().unwrap() = Some((Instant::now(), set.clone()));
        Ok(set)
    }

    /// The token's claims if GitHub (or the configured issuer) signed it for `audience`.
    pub async fn verify(&self, token: &str, audience: &str) -> Result<Claims, String> {
        let header = decode_header(token).map_err(|_| "not a GitHub OIDC token".to_string())?;
        if header.alg != Algorithm::RS256 {
            return Err("unexpected signing algorithm".into());
        }
        let kid = header.kid.ok_or("the token names no signing key")?;
        let mut set = self.jwks(false).await.map_err(|e| format!("couldn't fetch the issuer's keys: {e}"))?;
        if set.find(&kid).is_none() {
            // GitHub rotates keys; fetch again once.
            set = self.jwks(true).await.map_err(|e| format!("couldn't fetch the issuer's keys: {e}"))?;
        }
        let jwk = set.find(&kid).ok_or("signed with a key the issuer doesn't publish")?;
        let key = DecodingKey::from_jwk(jwk).map_err(|e| e.to_string())?;
        let mut v = Validation::new(Algorithm::RS256);
        v.set_issuer(&[&self.issuer]);
        v.set_audience(&[audience]);
        v.set_required_spec_claims(&["exp", "iss", "aud"]);
        v.leeway = 30;
        decode::<Claims>(token, &key, &v).map(|d| d.claims).map_err(|e| match e.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => "the token has expired".to_string(),
            jsonwebtoken::errors::ErrorKind::InvalidAudience => format!("the token is for another site (request it with audience {audience})"),
            jsonwebtoken::errors::ErrorKind::InvalidIssuer => "the token wasn't issued by GitHub Actions".to_string(),
            jsonwebtoken::errors::ErrorKind::InvalidSignature => "the token's signature doesn't check out".to_string(),
            _ => format!("the token isn't valid: {e}"),
        })
    }
}
