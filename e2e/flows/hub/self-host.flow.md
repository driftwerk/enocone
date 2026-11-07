---
area: Hosted site
tags: [hub, serve]
owner: enocone
priority: normal
stage: draft
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
  GitHub OAuth app id and secret (optional), and an email provider for
  sign-in links (Postmark or SMTP).
- With no GitHub app configured, sign-in is by email link only.
- The first person to sign in with the address in `--owner-email` becomes the
  site's owner; nobody else can sign in until invited.
- It serves plain HTTP on a local port and expects a proxy for HTTPS. The
  deployment guide shows Caddy.

## Given

1. **An empty data folder and an owner email**

## When

2. **enocone serve starts on port 8080**

## Then

3. **The owner signs in with an email link and creates the shop project**

4. **Restarting the server keeps the project and the owner's session**
