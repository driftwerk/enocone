---
area: Hosted site
tags: [hub, home]
owner: enocone
priority: normal
stage: approved
approved_by: Kingsley Hendrickse (2026-11-07)
---

# Someone new to enoc.one learns what enocone is and signs in from the home page

People reach enoc.one from a link in a pull request, a CI summary or a
colleague's message. Before they're asked to sign in they should see what
enocone is: flows written as documents, reviewed together, run against the
app and published from a laptop or CI. The sign-in card sits on the same page,
so a teammate who was invited is one click away from their project.

```flowchart
flowchart LR
  1[A visitor opens enoc.one] --> 2[Reads what enocone is] --> 3[Signs in from the card]
  1 --> 4[Already signed in: their projects]
```

## Rules

- Signed-out visitors get the home page at `/`; signed-in people get their
  projects there, as before.
- The home page needs no script and has the same security headers as every
  other page.
- "Continue with GitHub" appears only when the site has a GitHub app set up.
- Every example on the page is taken from enocone's own flows; nothing names
  a customer.

## Given

1. **A visitor opens the site without signing in**
   ```exec
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" GH_CLIENT=fixture-github-client sh "{{env.FIXTURES}}/hub/start.sh" 18186 "{{run.tmp}}/site"
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18186 "{{run.tmp}}/site" project:shop:Shop "invite:shop:sam+{{run.short_id}}@example.test:reviewer"
   expect: { stdout: seeded }
   ```
   ```browser
   - goto: "http://127.0.0.1:18186/"
   - expect: { heading: The spec your team reviews }
   - screenshot
   ```

## Then

2. **The page explains flows, review, publishing and self-hosting**
   ```browser
   - expect: { heading: A flow reads like a page }
   - expect: { heading: Comments stay }
   - expect: { heading: every run lands here }
   - expect: { heading: One command. }
   - expect: "A run is published to enoc.one from a laptop or from CI"
   - expect: { link: Self-host guide }
   ```
   ```http
   GET: "http://127.0.0.1:18186/"
   expect:
     status: 200
     headers:
       content-security-policy: { contains: "script-src 'none'" }
   ```

3. **Sam asks for a sign-in link from the home page's card**
   ```browser
   - expect: { link: Continue with GitHub }
   - fill: { label: Work email, value: "sam+{{run.short_id}}@example.test" }
   - click: Email me a sign-in link
   - expect: "a sign-in link is on its way"
   ```

4. **On a site without a GitHub app, the home page offers only the email link**
   ```exec
   run: |
     sh "{{env.FIXTURES}}/hub/stop.sh" 18186 && BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/start.sh" 18186 "{{run.tmp}}/site"
   expect: { stdout: "ready on port 18186" }
   ```
   ```browser
   - goto: "http://127.0.0.1:18186/"
   - expect: { button: Email me a sign-in link }
   - expect: { link: Continue with GitHub, hidden: true }
   ```
   ```exec
   run: |
     sh "{{env.FIXTURES}}/hub/stop.sh" 18186
   ```
