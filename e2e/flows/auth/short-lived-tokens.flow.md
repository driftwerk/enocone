---
area: Sign-in
tags: [auth]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-06)
---

# Long runs keep working when access tokens expire

Some systems issue access tokens that last two to
five minutes. A suite that signs in once would start failing partway through.
enocone should fetch a fresh token from the signed-in browser session before
the old one expires.

```flowchart
flowchart LR
  1[Tokens expire after 2 seconds] --> 2[Call, wait, call again] --> 3[The token was refreshed from the browser session]
  2 -.->|refresh fails| fail[A clear message, not a bare 401]
```

## Rules

- A browser auth profile's `token` can say `refresh = "60s"`. After that age,
  the next request re-reads the token from the session URL, using the saved
  browser session, with no new login.
- If the refresh fails, the step fails with a clear message rather than a
  confusing 401.

## Given

1. **The demo app issues tokens that expire after 2 seconds**
   ```exec
   # Copy the token project and check it is valid
   run: |
     cp -R "{{env.FIXTURES}}/tokens" "{{run.tmp}}/p" && "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate
   expect: { stderr: "1 flows OK" }
   ```

## When

2. **A flow calls the API, waits 3 seconds, and calls it again as the same profile**
   ```exec
   # Run the flow that calls the API twice
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --out "{{run.tmp}}/out"
   timeout: 60s
   expect: { exit: 0, stderr: "refreshing the token for `short`" }
   ```

## Then

3. **Both calls succeed, and the second used a different token**
   ```screen
   screen: token-refresh
   title: When a refresh fails, the step says why
   ```
   ```exec
   # Read the run's results
   run: |
     cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       $.flows[0].attempts[0].steps[0].records[0].status: 200
       $.flows[0].attempts[0].steps[2].records[0].status: 200
       $.flows[0].attempts[0].steps[0].captured[0][0]: first
       $.flows[0].attempts[0].steps[2].captured[0][0]: second
       $.flows[0].attempts[0].steps[2].records[0].checks[1].what: { matches: "^\\$.token_serial not = \\d+$" }
       $.flows[0].attempts[0].steps[2].records[0].checks[1].ok: true
   ```
