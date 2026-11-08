---
area: Getting started
tags: [hub, home]
owner: enocone
priority: normal
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-07)
---

# The home page shows how to start with an AI tool in three steps

Visitors who like what they read should know what to do next before they
leave the page. The home page gets a short "Get started" section: install,
connect, ask your AI tool for flows.

## Rules

- Three steps, each with the command to copy: `brew install
  driftwerk/tap/enocone`, `enocone init --name <project> --hub <this site>`,
  and an example request for the AI tool ("Design flows for the checkout
  feature").
- The site's own address goes into the second command, so self-hosted
  sites show theirs.
- It links to the full guide on GitHub.

## Given

1. **A visitor opens the home page**
   ```exec
   # Start a test site
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/start.sh" 18188 "{{run.tmp}}/site"
   expect: { stdout: "ready on port 18188" }
   ```
   ```browser
   - goto: "http://127.0.0.1:18188/#start"
   - expect: { heading: Three steps }
   ```

## Then

2. **They see the Get started section with the three steps and the site's address in the init command**
   ```browser
   - expect: { text: Install }
   - expect: "brew install enocone"
   - expect: { text: Connect your project }
   - expect: "--hub http://127.0.0.1:18188"
   - expect: { text: Ask for flows }
   - expect: "Design flows for the checkout"
   - expect: { link: enocone on GitHub }
   - screenshot
   ```
   ```exec
   # Stop the test site
   run: |
     sh "{{env.FIXTURES}}/hub/stop.sh" 18188
   ```
