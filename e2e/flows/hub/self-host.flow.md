---
area: Hosted site
tags: [hub, serve]
owner: enocone
priority: normal
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-07)
---

# Anyone can run their own enocone site with one command

enocone is open source, and so is the site. `enocone serve` runs everything
enoc.one runs, on one small machine, so a team can host its own if it
doesn't want its flows on someone else's server.

```flowchart
flowchart LR
  1[enocone serve with a data folder] --> 2[First visit creates the owner] --> 3[Owner adds a project and invites the team]
  1 --> 4[Caddy or any proxy adds HTTPS]
```

## Rules

- One binary, one data folder: SQLite for people, projects and comments; files
  for runs and screenshots. Backing up the folder backs up everything.
- Configuration comes from flags or environment variables: the public URL,
  GitHub OAuth app id and secret (optional), and Postmark for sign-in
  emails (optional: without it, links are written to the log).
- With no GitHub app configured, sign-in is by email link only.
- The first person to sign in with the address in `--owner-email` becomes the
  site's owner; nobody else can sign in until invited.
- It serves plain HTTP on a local port and expects a proxy for HTTPS. The
  deployment guide shows Caddy.

## Given

1. **An empty data folder and an owner email**
   ```exec
   run: |
     mkdir -p "{{run.tmp}}/site" && ls -A "{{run.tmp}}/site" | wc -l | tr -d ' '
   expect: { stdout: "0" }
   ```

## When

2. **enocone serve starts on a local port**
   ```exec
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/start.sh" 18181 "{{run.tmp}}/site" && curl -fsS http://127.0.0.1:18181/healthz
   expect: { stdout: "ready on port 18181" }
   ```

## Then

3. **The owner signs in with an email link and creates the shop project**
   ```browser
   - goto: "http://127.0.0.1:18181/login"
   - expect: { heading: Your team's flows }
   - fill: { label: Work email, value: "owner+{{run.short_id}}@example.test" }
   - click: Email me a sign-in link
   - expect: "a sign-in link is on its way"
   ```
   ```email
   to: "owner+{{run.short_id}}@example.test"
   subject: Your sign-in link
   within: 10s
   expect: { body: "expires in 15 minutes" }
   capture: { sign_in: { link: "/auth/email" } }
   ```
   ```browser
   - goto: "{{sign_in}}"
   - click: Sign in to
   - expect: { heading: Your projects }
   - fill: { label: Short name, value: shop }
   - fill: { label: Display name, value: Shop }
   - click: Create project
   - expect: { heading: Shop }
   - expect: "No runs yet"
   ```

4. **Restarting the server keeps the project and the owner's session**
   ```exec
   run: |
     sh "{{env.FIXTURES}}/hub/stop.sh" 18181 && BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/start.sh" 18181 "{{run.tmp}}/site"
   expect: { stdout: "ready on port 18181" }
   ```
   ```browser
   - goto: "http://127.0.0.1:18181/shop"
   - expect: { heading: Shop }
   - expect: { text: Signed in as }
   - screenshot
   ```
   ```exec
   run: |
     sh "{{env.FIXTURES}}/hub/stop.sh" 18181
   ```
