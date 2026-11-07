---
area: Hosted site
tags: [hub, members]
owner: enocone
priority: normal
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-07)
---

# Project owners invite the team, and everything is private by default

A team's flows describe unreleased features, so nothing on enoc.one is
public. An owner invites people by GitHub handle or email and gives each a
role.

```flowchart
flowchart LR
  1[Owner invites by handle or email] --> 2[Invitee signs in] --> 3[Sees only that project]
  4[Owner removes someone] --> 5[Their sessions and tokens stop working]
```

## Rules

- Roles: **owner** (manage members, link repositories), **member** (publish,
  comment, resolve, approve), **reviewer** (read, comment, approve).
- Every page and API call checks membership. Someone who is not a member gets
  a 404, not a 403, so project names don't leak.
- A project can also be granted to a GitHub organisation or team, with one
  role for all its members.
- A project has at least one owner.
- Removing someone revokes their sessions and their publish tokens for that
  project straight away.

## Given

1. **Kingsley owns the shop project**
   ```exec
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/start.sh" 18183 "{{run.tmp}}/site" >/dev/null
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18183 "{{run.tmp}}/site" project:shop:Shop project:billing:Billing "invite:shop:priya+{{run.short_id}}@example.test:member" "invite:billing:olu+{{run.short_id}}@example.test:member" "token:priya+{{run.short_id}}@example.test" "link:owner+{{run.short_id}}@example.test"
   expect: { stdout: seeded }
   capture:
     priya_token: { regex: "(enc_[0-9a-f]+)" }
     owner_link: { regex: "(http://\\S+/auth/email\\?token=\\w+)" }
   ```
   ```browser
   - goto: "{{owner_link}}"
   - click: Sign in to
   - goto: "http://127.0.0.1:18183/shop/members"
   - expect: { heading: Who can see Shop }
   - expect: "owner+{{run.short_id}}@example.test"
   ```

## When

2. **Kingsley invites sam@company.com as a reviewer**
   ```screen
   screen: hub-members
   title: Members
   path: /shop/members
   ```
   ```browser
   - fill: { label: Invite a person or a GitHub team, value: "sam+{{run.short_id}}@example.test" }
   - select: { label: Role, option: Reviewer }
   - click: Send invite
   - expect: "sam+{{run.short_id}}@example.test"
   - screenshot
   ```
   ```email
   to: "sam+{{run.short_id}}@example.test"
   subject: You're invited to Shop
   within: 10s
   expect: { body: "as a reviewer" }
   ```

## Then

3. **Sam can read and comment on shop but not publish to it**
   ```exec
   run: |
     cp -R "{{env.FIXTURES}}/hub-review" "{{run.tmp}}/p" && "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run >/dev/null 2>&1
     env -u GITHUB_ACTIONS -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_ID_TOKEN_REQUEST_TOKEN ENOCONE_HUB_TOKEN="{{priya_token}}" "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" publish --url http://127.0.0.1:18183 --to shop >/dev/null
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18183 "{{run.tmp}}/site" "token:sam+{{run.short_id}}@example.test" "link:sam+{{run.short_id}}@example.test"
   capture:
     sam_token: { regex: "(enc_[0-9a-f]+)" }
     sam_link: { regex: "(http://\\S+/auth/email\\?token=\\w+)" }
   ```
   ```http
   POST: "http://127.0.0.1:18183/api/projects/shop/runs"
   headers: { Authorization: "Bearer {{sam_token}}" }
   json: { results: {} }
   expect:
     status: 403
     json:
       $.error: { contains: "Reviewers can't publish" }
   ```
   ```browser
   - goto: "http://127.0.0.1:18183/"
   - click: Sign out
   - goto: "{{sam_link}}"
   - click: Sign in to
   - goto: "http://127.0.0.1:18183/shop/review/quotes.accept-from-email"
   - fill: { label: Your comment, value: "Looks right to me." }
   - click: { button: Comment }
   - expect: "Open comments (1)"
   - expect: { button: Resolve, hidden: true }
   ```

4. **Someone who isn't a member gets “not found” for /shop**
   ```exec
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18183 "{{run.tmp}}/site" "link:olu+{{run.short_id}}@example.test"
   capture: { olu_link: { regex: "(http://\\S+/auth/email\\?token=\\w+)" } }
   ```
   ```browser
   - goto: "http://127.0.0.1:18183/"
   - click: Sign out
   - goto: "{{olu_link}}"
   - click: Sign in to
   - expect: { link: Billing }
   - goto: "http://127.0.0.1:18183/shop"
   - expect: { heading: Not found }
   ```

5. **After Kingsley removes Priya, her publish token is refused**
   ```http
   GET: "http://127.0.0.1:18183/api/whoami"
   headers: { Authorization: "Bearer {{priya_token}}" }
   expect:
     status: 200
     json:
       $.projects[0]: shop
   ```
   ```exec
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18183 "{{run.tmp}}/site" "remove:shop:email:priya+{{run.short_id}}@example.test"
   expect: { stdout: seeded }
   ```
   ```http
   POST: "http://127.0.0.1:18183/api/projects/shop/runs"
   headers: { Authorization: "Bearer {{priya_token}}" }
   json: { results: {} }
   expect: { status: 404 }
   ```
   ```exec
   run: |
     sh "{{env.FIXTURES}}/hub/stop.sh" 18183
   ```
