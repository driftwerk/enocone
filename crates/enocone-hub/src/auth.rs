//! Secrets and sessions: random tokens, stored only as SHA-256 hashes.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, header};
use sha2::{Digest, Sha256};

pub const SESSION_COOKIE: &str = "enocone_session";
pub const SESSION_DAYS: i64 = 30;
pub const LINK_MINUTES: i64 = 15;

/// 32 random bytes, URL-safe.
pub fn random_token() -> String {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("the OS random number generator");
    hex(&b)
}

pub fn hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    for v in headers.get_all(header::COOKIE) {
        for part in v.to_str().ok()?.split(';') {
            if let Some((k, val)) = part.trim().split_once('=')
                && k == name
            {
                return Some(val.to_string());
            }
        }
    }
    None
}

pub fn bearer(headers: &HeaderMap) -> Option<String> {
    headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ").map(|s| s.trim().to_string())
}

pub fn session_cookie(token: &str, secure: bool) -> String {
    format!("{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}", SESSION_DAYS * 86400, if secure { "; Secure" } else { "" })
}

pub fn clear_cookie(secure: bool) -> String {
    format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}", if secure { "; Secure" } else { "" })
}

/// A simple sliding-window limiter: at most `max` hits per key in `window`.
pub struct Limiter {
    max: usize,
    window: Duration,
    hits: Mutex<HashMap<String, Vec<Instant>>>,
}

impl Limiter {
    pub fn new(max: usize, window: Duration) -> Limiter {
        Limiter { max, window, hits: Mutex::new(HashMap::new()) }
    }
    pub fn allow(&self, key: &str) -> bool {
        let mut m = self.hits.lock().unwrap();
        let now = Instant::now();
        let v = m.entry(key.to_string()).or_default();
        v.retain(|t| now.duration_since(*t) < self.window);
        if v.len() >= self.max {
            return false;
        }
        v.push(now);
        if m.len() > 10_000 {
            m.retain(|_, v| v.iter().any(|t| now.duration_since(*t) < self.window));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_and_hashed() {
        let a = random_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, random_token());
        assert_eq!(hash(&a).len(), 64);
        assert_ne!(hash(&a), a);
    }

    #[test]
    fn limiter_caps_hits() {
        let l = Limiter::new(2, Duration::from_secs(60));
        assert!(l.allow("x") && l.allow("x"));
        assert!(!l.allow("x"));
        assert!(l.allow("y"));
    }

    #[test]
    fn reads_cookies() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, "a=1; enocone_session=abc".parse().unwrap());
        assert_eq!(cookie(&h, SESSION_COOKIE).as_deref(), Some("abc"));
    }
}
