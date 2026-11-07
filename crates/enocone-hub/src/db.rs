//! SQLite storage: people, sign-in links, sessions, projects, members,
//! publish tokens, runs and comments. One file in the data folder.

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension, params};

#[derive(Clone)]
pub struct Db(Arc<Mutex<Connection>>);

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY,
  email TEXT UNIQUE,
  github_login TEXT UNIQUE,
  github_id INTEGER UNIQUE,
  name TEXT,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS user_teams (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  team TEXT NOT NULL,
  PRIMARY KEY (user_id, team)
);
CREATE TABLE IF NOT EXISTS email_links (
  token_hash TEXT PRIMARY KEY,
  email TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  used_at INTEGER,
  next TEXT
);
CREATE TABLE IF NOT EXISTS sessions (
  token_hash TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS projects (
  id INTEGER PRIMARY KEY,
  slug TEXT UNIQUE NOT NULL,
  name TEXT NOT NULL,
  repo TEXT,
  created_at TEXT NOT NULL
);
-- kind: email | github | team; subject is lower case (an address, a login, or org/team)
CREATE TABLE IF NOT EXISTS members (
  project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  subject TEXT NOT NULL,
  role TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (project_id, kind, subject)
);
CREATE TABLE IF NOT EXISTS tokens (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  token_hash TEXT UNIQUE NOT NULL,
  created_at TEXT NOT NULL,
  last_used_at TEXT
);
CREATE TABLE IF NOT EXISTS runs (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  run_id TEXT NOT NULL,
  executed INTEGER NOT NULL,
  prototype INTEGER NOT NULL,
  source TEXT NOT NULL,
  published_by TEXT NOT NULL,
  branch TEXT,
  commit_sha TEXT,
  summary TEXT NOT NULL,
  flows TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS runs_project ON runs(project_id, created_at);
CREATE TABLE IF NOT EXISTS comments (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  flow_id TEXT NOT NULL,
  step_key TEXT,
  quote TEXT,
  parent_id INTEGER REFERENCES comments(id) ON DELETE CASCADE,
  author_id INTEGER NOT NULL REFERENCES users(id),
  text TEXT NOT NULL,
  flow_hash TEXT,
  created_at TEXT NOT NULL,
  resolved_at TEXT,
  resolved_by INTEGER REFERENCES users(id)
);
CREATE INDEX IF NOT EXISTS comments_flow ON comments(project_id, flow_id);
"#;

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Db> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Db(Arc::new(Mutex::new(conn))))
    }

    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> anyhow::Result<T> {
        let conn = self.0.lock().map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        Ok(f(&conn)?)
    }
}

#[derive(Debug, Clone)]
pub struct User {
    pub id: i64,
    pub email: Option<String>,
    pub github_login: Option<String>,
    pub name: Option<String>,
}

impl User {
    pub fn label(&self) -> String {
        self.name.clone().or_else(|| self.github_login.clone()).or_else(|| self.email.clone()).unwrap_or_else(|| format!("user {}", self.id))
    }
}

pub fn user_by_id(c: &Connection, id: i64) -> rusqlite::Result<Option<User>> {
    c.query_row("SELECT id, email, github_login, name FROM users WHERE id = ?1", [id], |r| Ok(User { id: r.get(0)?, email: r.get(1)?, github_login: r.get(2)?, name: r.get(3)? })).optional()
}

pub fn user_by_email(c: &Connection, email: &str) -> rusqlite::Result<Option<User>> {
    c.query_row("SELECT id, email, github_login, name FROM users WHERE email = ?1", [email], |r| Ok(User { id: r.get(0)?, email: r.get(1)?, github_login: r.get(2)?, name: r.get(3)? })).optional()
}

pub fn user_for_email(c: &Connection, email: &str) -> rusqlite::Result<User> {
    if let Some(u) = user_by_email(c, email)? {
        return Ok(u);
    }
    c.execute("INSERT INTO users (email, created_at) VALUES (?1, ?2)", params![email, now()])?;
    Ok(User { id: c.last_insert_rowid(), email: Some(email.to_string()), github_login: None, name: None })
}

#[derive(Debug, Clone)]
pub struct Project {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub repo: Option<String>,
}

pub fn project_by_slug(c: &Connection, slug: &str) -> rusqlite::Result<Option<Project>> {
    c.query_row("SELECT id, slug, name, repo FROM projects WHERE slug = ?1", [slug], |r| Ok(Project { id: r.get(0)?, slug: r.get(1)?, name: r.get(2)?, repo: r.get(3)? })).optional()
}

pub fn all_projects(c: &Connection) -> rusqlite::Result<Vec<Project>> {
    let mut st = c.prepare("SELECT id, slug, name, repo FROM projects ORDER BY slug")?;
    st.query_map([], |r| Ok(Project { id: r.get(0)?, slug: r.get(1)?, name: r.get(2)?, repo: r.get(3)? }))?.collect()
}

pub fn teams_of(c: &Connection, user_id: i64) -> rusqlite::Result<Vec<String>> {
    let mut st = c.prepare("SELECT team FROM user_teams WHERE user_id = ?1")?;
    st.query_map([user_id], |r| r.get(0))?.collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Reviewer,
    Member,
    Owner,
}

impl Role {
    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "reviewer" => Some(Role::Reviewer),
            "member" => Some(Role::Member),
            "owner" => Some(Role::Owner),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Reviewer => "reviewer",
            Role::Member => "member",
            Role::Owner => "owner",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Role::Reviewer => "Reviewer",
            Role::Member => "Member",
            Role::Owner => "Owner",
        }
    }
}

/// The highest role `user` has on `project` through an email, GitHub login or team grant.
pub fn role_of(c: &Connection, project_id: i64, user: &User) -> rusqlite::Result<Option<Role>> {
    let mut best: Option<Role> = None;
    let mut consider = |kind: &str, subject: &str| -> rusqlite::Result<()> {
        let role: Option<String> =
            c.query_row("SELECT role FROM members WHERE project_id = ?1 AND kind = ?2 AND subject = ?3", params![project_id, kind, subject.to_lowercase()], |r| r.get(0)).optional()?;
        if let Some(r) = role.as_deref().and_then(Role::parse) {
            best = best.max(Some(r));
        }
        Ok(())
    };
    if let Some(e) = &user.email {
        consider("email", e)?;
    }
    if let Some(l) = &user.github_login {
        consider("github", l)?;
    }
    for t in teams_of(c, user.id)? {
        consider("team", &t)?;
    }
    Ok(best)
}

/// Is this address allowed to ask for a sign-in link: invited somewhere, or the site owner.
pub fn email_may_sign_in(c: &Connection, email: &str) -> rusqlite::Result<bool> {
    let n: i64 = c.query_row("SELECT COUNT(*) FROM members WHERE kind = 'email' AND subject = ?1", [email], |r| r.get(0))?;
    if n > 0 {
        return Ok(true);
    }
    // Someone who already signed in with GitHub and has this email may be a member by login or team.
    if let Some(u) = user_by_email(c, email)? {
        for p in all_projects(c)? {
            if role_of(c, p.id, &u)?.is_some() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub fn epoch() -> i64 {
    chrono::Utc::now().timestamp()
}
