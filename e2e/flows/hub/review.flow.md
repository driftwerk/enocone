---
area: Hosted site
tags: [hub, review, comments]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-07)
---

# Reviewers comment beside a draft flow

Drafts are written to be argued about. On enoc.one a reviewer selects a
sentence or a step and leaves a comment in the margin, the way they would on a
document. Approval stays in the flow file, in git, so the spec and the code
stay in one place; approving on the site comes later.

```flowchart
flowchart LR
  1[A draft is published] --> 2[Reviewers comment in the margin] --> 3[The author edits and publishes again]
  3 --> 4[Comments stay on their text or are marked outdated] --> 5[Settled comments are resolved]
  5 -.->|agreed| done[The author sets stage: approved in git]
```

## Rules

- Comments attach to a step or to a quoted passage of the flow's text, and
  belong to the flow's id, not to a run.
- A new version of the flow keeps a comment where its text is unchanged, and
  marks it **outdated** otherwise, still readable.
- Anyone invited can comment; members can resolve.
- Approving happens in the flow file, as today: the author sets
  `stage: approved` and `approved_by:` and commits. Authors may approve their
  own drafts.
- Later (not in this version): an Approve button on the site, and a CI job
  that opens a pull request writing the approvals into the flow files.
- Comments also appear in the HTML report when it's downloaded from the site.

## Given

1. **Priya has published the draft “Client accepts a quote from the email”**
   ```exec
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/start.sh" 18185 "{{run.tmp}}/site" >/dev/null
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18185 "{{run.tmp}}/site" project:shop:Shop "invite:shop:priya+{{run.short_id}}@example.test:member" "invite:shop:sam+{{run.short_id}}@example.test:reviewer" "token:priya+{{run.short_id}}@example.test"
   expect: { stdout: seeded }
   capture: { priya_token: { regex: "(enc_[0-9a-f]+)" } }
   ```
   ```exec
   run: |
     cp -R "{{env.FIXTURES}}/hub-review" "{{run.tmp}}/p"
     printf '\n[hub]\nurl = "http://127.0.0.1:18185"\nproject = "shop"\n' >> "{{run.tmp}}/p/enocone.toml"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run >/dev/null 2>&1
     env -u GITHUB_ACTIONS -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_ID_TOKEN_REQUEST_TOKEN ENOCONE_HUB_TOKEN="{{priya_token}}" "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" publish
   expect: { stdout: "/shop/runs/" }
   ```

## When

2. **Sam comments on the rule about the 14-day expiry, and Priya replies**
   ```screen
   screen: hub-review
   title: Review page
   path: /shop/review/quotes.accept-from-email
   ```
   ```exec
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18185 "{{run.tmp}}/site" "link:sam+{{run.short_id}}@example.test" "link:priya+{{run.short_id}}@example.test"
   capture:
     sam_link: { regex: "(http://\\S+/auth/email\\?token=\\w+)" }
     priya_link: { regex: "auth/email\\?token=\\w+\\n(http://\\S+/auth/email\\?token=\\w+)" }
   ```
   ```browser
   - goto: "{{sam_link}}"
   - click: Sign in to
   - goto: "http://127.0.0.1:18185/shop/review"
   - click: Client accepts a quote from the email
   - expect: { heading: Client accepts a quote from the email }
   - fill: { label: Passage you're commenting on, value: "expires after 14 days" }
   - fill: { label: Your comment, value: "14 days is too short for conveyancing; clients often wait for a mortgage offer." }
   - click: { button: Comment }
   - expect: "Open comments (1)"
   - expect: { button: Resolve, hidden: true }
   - click: Sign out
   - goto: "{{priya_link}}"
   - click: Sign in to
   - goto: "http://127.0.0.1:18185/shop/review/quotes.accept-from-email"
   - fill: { label: "Reply to sam+{{run.short_id}}@example.test", value: "Agreed, I'll make it 30 days." }
   - click: { button: Reply }
   - expect: "Agreed, I'll make it 30 days."
   - screenshot
   ```

3. **Priya changes the rule to 30 days and publishes again**
   ```exec
   run: |
     sed -i.bak 's/expires after 14 days/expires after 30 days/' "{{run.tmp}}/p/flows/quotes/accept-from-email.flow.md"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run >/dev/null 2>&1
     env -u GITHUB_ACTIONS -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_ID_TOKEN_REQUEST_TOKEN ENOCONE_HUB_TOKEN="{{priya_token}}" "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" publish
   expect: { stdout: "/shop/runs/" }
   ```

## Then

4. **Sam's comment shows as outdated beside the new rule, still readable**
   ```browser
   - goto: "http://127.0.0.1:18185/shop/review/quotes.accept-from-email"
   - expect: "expires after 30 days."
   - expect: { text: Outdated }
   - expect: "14 days is too short for conveyancing"
   - screenshot
   ```

5. **Priya resolves the comment and it moves to the resolved list**
   ```browser
   - click: { button: Resolve }
   - expect: "Open comments (0)"
   - expect: "Resolved (1)"
   ```
   ```exec
   run: |
     sh "{{env.FIXTURES}}/hub/stop.sh" 18185
   ```
