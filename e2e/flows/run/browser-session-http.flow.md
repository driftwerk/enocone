---
area: Running flows
tags: [http, browser, auth]
owner: enocone
priority: normal
stage: approved
approved_by: Kingsley Hendrickse (2026-11-12)
---

# An http block can send its request with the signed-in browser's cookies

Some endpoints accept only the browser's session cookie, not a token: an
API route behind a web sign-in, for example. A flow can't call them today.
`http` blocks carry a profile's token, and a browser block can't make a
request and capture its answer. An `http` block that borrows the browser's
cookies can check and capture such an endpoint like any other.

```flowchart
flowchart LR
  1[The demo app signs people in with a cookie] --> 2[A flow signs in through the browser]
  2 --> 3[An http block with session: browser gets the answer]
  3 --> 4[The report masks the cookie]
  2 -.->|without session: browser| 5[The same request is answered as signed out]
  A[No browser yet] -.->|rejected, saying how to fix it| 6[An error before the request is sent]
```

## Rules

- An `http` block with `session: browser` sends its request with the
  cookies of a signed-in browser:
  - with `auth: <profile>` naming a browser sign-in profile, that
    profile's saved sign-in;
  - without `auth:`, the flow's own browser, as it is at that moment (after
    the browser steps before it).
- Only the cookies the browser itself would send to that URL go with the
  request (matching domain, path and `Secure`, and not expired).
- With `session: browser`, a profile's cookies are used and its token
  header isn't sent.
- When there is no browser to borrow from (no browser step has run, and no
  browser profile is named), the step fails before sending anything, saying
  to add a browser step first or name a browser profile with `auth:`.
- The `Cookie` header and every cookie value are masked in the report,
  results and published runs, like other secrets.
- Everything else works as in any `http` block: `expect`, `capture`,
  `eventually`, tables and `For each:`.

## Given

1. **The demo app signs people in with a session cookie, and its session endpoint answers only to that cookie**

## When

2. **A flow signs in through the browser, then an http block with `session: browser` asks the session endpoint for the signed-in token**

## Then

3. **The request carries the browser's cookie, is answered 200, and the token is captured**

4. **The report shows the request with its Cookie header and the cookie's value masked**

5. **The same http block without `session: browser` is answered as signed out**

6. **An http block with `session: browser` and no browser to borrow from fails before sending anything, saying how to fix it**
