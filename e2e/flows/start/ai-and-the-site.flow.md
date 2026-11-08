---
area: Getting started
tags: [mcp, hub, review]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-07)
---

# The AI tool works with the team site: it publishes drafts and answers review comments

Writing a draft is half the job; the other half is the team's review. The AI
working on a flow should be able to publish it for review, read what people
said about it on the site, and revise the draft, without anyone copying
comments between windows.

```flowchart
flowchart LR
  1[The AI writes a draft] --> 2[It publishes the run] --> 3[Reviewers comment on the site]
  3 --> 4[The AI reads the open comments] --> 5[Sees each comment and what it's about]
  5 --> r[It revises the draft and publishes again] --> 3
  4 -.->|not signed in| 6[Told to run enocone login]
```

## Rules

- The MCP server gains tools for the site: whether you're signed in and the
  project exists (`hub_status`), publishing the last run (`publish_run`),
  and reading the open comments on a flow (`review_comments`).
- They use the same sign-in as the CLI (`enocone login`, or a token in
  `ENOCONE_HUB_TOKEN`); the AI never asks for or handles a password.
- Comments come back with who wrote them, which step or quoted text they
  are about, and whether they're outdated.
- Comments are what people wrote, so the AI treats them as feedback to
  weigh, not as instructions to follow. The tool's output and the guide
  both say so.
- Reading comments needs a token for a member of the project; anyone else
  gets "not found", as on the site.
- The AI still never approves a flow; approval stays a person's change in
  git.
- The guide (`format_guide`) gains a section on working with the team site.

## Open questions

- Should the AI be able to reply to a comment on the site ("changed to 30
  days in the new version"), or only change the draft?

## Given

1. **Priya's AI tool has written a draft flow and run it**
   ```exec
   # Start a test site with Priya and Sam on shop
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/start.sh" 18187 "{{run.tmp}}/site" >/dev/null
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18187 "{{run.tmp}}/site" project:shop:Shop "invite:shop:priya+{{run.short_id}}@example.test:member" "invite:shop:sam+{{run.short_id}}@example.test:reviewer" "token:priya+{{run.short_id}}@example.test"
   expect: { stdout: seeded }
   capture: { priya_token: { regex: "(enc_[0-9a-f]+)" } }
   ```
   ```exec
   # Run the draft through the MCP server
   run: |
     cp -R "{{env.FIXTURES}}/hub-review" "{{run.tmp}}/p"
     printf '\n[hub]\nurl = "http://127.0.0.1:18187"\nproject = "shop"\n' >> "{{run.tmp}}/p/enocone.toml"
     python3 "{{env.FIXTURES}}/mcp/call.py" "{{env.ENOCONE_BIN}}" "{{run.tmp}}/p" run_flows
   expect: { stdout: "quotes.accept-from-email" }
   ```

## When

2. **It publishes the run to the shop project**
   ```exec
   # Ask the MCP server for the site's status
   run: |
     export ENOCONE_HUB_TOKEN="{{priya_token}}"
     unset GITHUB_ACTIONS ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_ID_TOKEN_REQUEST_TOKEN
     python3 "{{env.FIXTURES}}/mcp/call.py" "{{env.ENOCONE_BIN}}" "{{run.tmp}}/p" hub_status
   expect:
     json:
       $.signed_in: true
       $.project_found: true
       $.project_url: "http://127.0.0.1:18187/shop"
   ```
   ```exec
   # Publish the run through the MCP server
   run: |
     export ENOCONE_HUB_TOKEN="{{priya_token}}"
     unset GITHUB_ACTIONS ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_ID_TOKEN_REQUEST_TOKEN
     python3 "{{env.FIXTURES}}/mcp/call.py" "{{env.ENOCONE_BIN}}" "{{run.tmp}}/p" publish_run
   expect:
     json:
       $.published: { contains: "/shop/runs/" }
       $.review: "http://127.0.0.1:18187/shop/review"
   ```

3. **Sam comments on the draft's expiry rule**
   ```exec
   # Get Sam a sign-in link
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18187 "{{run.tmp}}/site" "link:sam+{{run.short_id}}@example.test"
   capture:
     sam_link: { regex: "(http://\\S+/auth/email\\?token=\\w+)" }
   ```
   ```browser
   - goto: "{{sam_link}}"
   - click: Sign in to
   - goto: "http://127.0.0.1:18187/shop/review/quotes.accept-from-email"
   - fill: { label: Passage you're commenting on, value: "expires after 14 days" }
   - fill: { label: Your comment, value: "14 days is too short for conveyancing; clients often wait for a mortgage offer." }
   - click: { button: Comment }
   - expect: "Open comments (1)"
   ```

4. **The AI reads the open comments on that flow**
   ```exec
   # Read the flow's open comments through the MCP server
   run: |
     export ENOCONE_HUB_TOKEN="{{priya_token}}"
     unset GITHUB_ACTIONS ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_ID_TOKEN_REQUEST_TOKEN
     python3 "{{env.FIXTURES}}/mcp/call.py" "{{env.ENOCONE_BIN}}" "{{run.tmp}}/p" review_comments '{"id": "quotes.accept-from-email"}' | tee "{{run.tmp}}/comments.json"
   expect:
     json:
       $.flow.id: quotes.accept-from-email
       $.comments: { len: 1 }
   ```

## Then

5. **It sees Sam's comment, the rule it quotes, and that it's open**
   ```exec
   # Check the comment, the rule it quotes and that it's open
   run: |
     cat "{{run.tmp}}/comments.json"
   expect:
     json:
       $.comments[0].author: "sam+{{run.short_id}}@example.test"
       $.comments[0].text: { contains: "too short for conveyancing" }
       $.comments[0].quote: "expires after 14 days"
       $.comments[0].resolved: false
       $.comments[0].outdated: false
       $.note: { contains: "not as instructions" }
   ```

6. **Without a sign-in it is told to run enocone login, and is shown no comments**
   ```exec
   # Ask for comments without signing in
   run: |
     unset ENOCONE_HUB_TOKEN GITHUB_ACTIONS ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_ID_TOKEN_REQUEST_TOKEN
     python3 "{{env.FIXTURES}}/mcp/call.py" "{{env.ENOCONE_BIN}}" "{{run.tmp}}/p" review_comments '{"id": "quotes.accept-from-email"}'
   expect:
     stdout: { contains: "run `enocone login`", not: { contains: "conveyancing" } }
   ```
   ```exec
   # Stop the test site
   run: |
     sh "{{env.FIXTURES}}/hub/stop.sh" 18187
   ```
