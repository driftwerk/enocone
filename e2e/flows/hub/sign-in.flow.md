---
area: Hosted site
tags: [hub, sign-in]
owner: enocone
priority: high
stage: approved
approved_by: Kingsley Hendrickse (2026-11-07)
---

# Teammates sign in with GitHub or a one-time email link

Flow documents are for the whole team, not just developers. Developers already
have GitHub accounts; product owners, testers and clients often don't. So
enoc.one offers both: GitHub for people who have it, and a one-time email
link for everyone else. Nobody manages a password.

```storyboard
- { from: hub-login, to: hub-link-sent, step: 2, on: { click: Email me a sign-in link } }
- { from: hub-link-sent, to: hub-project, step: 3, on: { click: Open sign-in link } }
```

```flowchart
flowchart LR
  1[Someone opens enoc.one] --> gh{GitHub account?}
  gh -->|yes| 4[Signs in with GitHub]
  gh -->|no| 2[Asks for an email link] --> 3[Opens the link once]
  4 --> in[Sees the projects they were invited to]
  3 --> in
  2 -.->|not invited| 5[Same message, nothing sent]
```

## Rules

- Only people invited to at least one project can sign in. Signing in never
  creates access on its own.
- An owner can also grant a project to a GitHub organisation or team
  (everyone in `acme/qa` can see `shop`). Membership is checked with GitHub at
  each sign-in, so people who leave the team lose access.
- An email link works once, expires after 15 minutes, and is stored only as a
  hash. Asking for links is rate limited per address and per IP.
- The page says the same thing whether or not the address is invited, so it
  can't be used to discover who is on a team.
- GitHub sign-in asks for the user's identity, email and organisation/team
  membership (`read:user`, `user:email`, `read:org`), so team grants can be
  checked; never repository access.
- Sessions last 30 days in an HttpOnly, Secure, SameSite=Lax cookie.

## Given

1. **Sam, a reviewer invited by email, opens enoc.one**
   ```screen
   screen: hub-login
   title: Sign in
   path: /login
   ```
   ```exec
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" GH_CLIENT=fixture-github-client sh "{{env.FIXTURES}}/hub/start.sh" 18182 "{{run.tmp}}/site"
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18182 "{{run.tmp}}/site" project:shop:Shop project:billing:Billing "invite:shop:sam+{{run.short_id}}@example.test:reviewer" "invite:shop:acme/qa:member"
   expect: { stdout: seeded }
   ```
   ```browser
   - goto: "http://127.0.0.1:18182/"
   - expect: { heading: Welcome }
   - expect: { button: Email me a sign-in link }
   - screenshot
   ```

## When

2. **Sam asks for a sign-in link**
   ```screen
   screen: hub-link-sent
   title: Link sent
   path: /login/sent
   ```
   ```browser
   - fill: { label: Work email, value: "sam+{{run.short_id}}@example.test" }
   - click: Email me a sign-in link
   - expect: "a sign-in link is on its way"
   - expect: "expires in 15 minutes"
   - screenshot
   ```

3. **Sam opens the link from the email**
   ```email
   to: "sam+{{run.short_id}}@example.test"
   subject: Your sign-in link
   within: 10s
   capture: { sign_in: { link: "/auth/email" } }
   ```
   ```browser
   - goto: "{{sign_in}}"
   - expect: "Continue as sam+"
   - click: Sign in to
   ```

## Then

4. **Sam sees the shop project and nothing else**
   ```screen
   screen: hub-project
   title: Project page
   path: /shop
   ```
   ```browser
   - expect: { heading: Your projects }
   - expect: { link: Shop }
   - expect: { link: Billing, hidden: true }
   - screenshot
   ```
   ```browser
   - goto: "http://127.0.0.1:18182/billing"
   - expect: { heading: Not found }
   - expect: "you don't have access to it"
   ```

5. **Opening the same link again shows that it has been used**
   ```browser
   - goto: "{{sign_in}}"
   - expect: { heading: This link has already been used }
   - expect: { link: Get a new link }
   ```

6. **Asking for a link for an address nobody invited shows the same message and sends nothing**
   ```browser
   - goto: "http://127.0.0.1:18182/"
   - click: Sign out
   - fill: { label: Work email, value: "stranger+{{run.short_id}}@example.test" }
   - click: Email me a sign-in link
   - expect: "a sign-in link is on its way"
   ```
   ```http
   GET: "{{env.WEB}}/postmark/messages/outbound"
   headers: { X-Postmark-Server-Token: fixture-postmark-token }
   query: { recipient: "stranger+{{run.short_id}}@example.test" }
   expect:
     status: 200
     json:
       $.TotalCount: 0
   ```

7. **Dev, a member of the GitHub team acme/qa that was granted shop, signs in with GitHub and sees shop**
   ```browser
   - goto: "http://127.0.0.1:18182/login"
   - click: Continue with GitHub
   - expect: "Scopes requested: read:user user:email read:org"
   - fill: { label: GitHub login, value: "dev-{{run.short_id}}" }
   - fill: { label: Teams, value: "acme/qa, acme/design" }
   - click: Authorize
   - expect: { heading: Your projects }
   - expect: { link: Shop }
   - expect: { link: Billing, hidden: true }
   - expect: "Signed in as dev-{{run.short_id}}"
   - click: Sign out
   - click: Continue with GitHub
   - fill: { label: GitHub login, value: "outsider-{{run.short_id}}" }
   - fill: { label: Teams, value: "acme/design" }
   - click: Authorize
   - expect: { heading: "@outsider-{{run.short_id}} isn't on any project here" }
   ```
   ```exec
   run: |
     sh "{{env.FIXTURES}}/hub/stop.sh" 18182
   ```
