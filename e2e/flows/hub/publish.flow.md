---
area: Hosted site
tags: [hub, publish, ci]
owner: enocone
priority: high
stage: approved
approved_by: Kingsley Hendrickse (2026-11-07)
---

# A run is published to enoc.one from a laptop or from CI

A report on someone's laptop or buried in CI artifacts is hard to discuss. `enocone
publish` sends a run to the team's site, where everyone sees the same pages.
From GitHub Actions it needs no stored secret: GitHub vouches for the workflow.

```flowchart
flowchart LR
  1[enocone login on a laptop] --> 2[enocone publish] --> 4[The run appears on the project page]
  3[CI with publish: true] --> 4
  4 --> 5[Docs and drafts update from the same upload]
  2 -.->|not a member| 6[Refused, saying who can invite]
```

## Rules

- Publishing sends `results.json` and screenshots, never HTML. The site draws
  every page itself, so a published run can't put scripts on enoc.one.
  Mockups stay in sandboxed frames, as in the report.
- Uploads are capped (50 MB per run) and only PNG screenshots are accepted
  beside the results.
- `enocone login` stores a personal token in the OS keychain where there is
  one (macOS, Windows, Secret Service), otherwise in
  `~/.config/enocone/credentials` readable only by the user. CI uses GitHub
  OIDC or `ENOCONE_HUB_TOKEN`. Tokens can be listed and revoked on the site.
- In GitHub Actions, the action requests an OIDC token for audience
  `https://enoc.one`. The site checks GitHub's signature, the issuer,
  expiry and audience, and accepts it only for projects linked to that
  repository (`acme/shop`). Forks' pull requests are refused.
- Runs are kept for 30 days, then deleted with their screenshots. Flow
  documents and comments are kept.
- Secrets are already masked in results; the site never sees `.enocone.env`.
- The project is named in `enocone.toml` under `[hub]`:
  `url = "https://enoc.one"`, `project = "shop"`.

## Given

1. **Priya is a member of shop and has run enocone login**
   ```exec
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/start.sh" 18184 "{{run.tmp}}/site" >/dev/null
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18184 "{{run.tmp}}/site" project:shop:Shop:acme/shop "invite:shop:priya+{{run.short_id}}@example.test:member" "token:priya+{{run.short_id}}@example.test" "link:priya+{{run.short_id}}@example.test"
   expect: { stdout: seeded }
   capture:
     priya_token: { regex: "(enc_[0-9a-f]+)" }
     priya_link: { regex: "(http://\\S+/auth/email\\?token=\\w+)" }
   ```
   ```exec
   run: |
     printf %s "{{priya_token}}" | ENOCONE_CREDENTIALS_FILE="{{run.tmp}}/credentials" "{{env.ENOCONE_BIN}}" login --with-token --url http://127.0.0.1:18184
     ls -l "{{run.tmp}}/credentials"
   expect:
     stdout: { matches: "signed in to http://127.0.0.1:18184 as priya\\+\\w+@example.test\\n-rw-------" }
   ```

## When

2. **Priya runs the prototype and publishes it**
   ```screen
   screen: hub-publish
   title: Publishing from the command line and CI
   ```
   ```exec
   run: |
     cp -R "{{env.FIXTURES}}/prototype" "{{run.tmp}}/p"
     printf '\n[hub]\nurl = "http://127.0.0.1:18184"\nproject = "shop"\n' >> "{{run.tmp}}/p/enocone.toml"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --prototype >/dev/null 2>&1
     ENOCONE_CREDENTIALS_FILE="{{run.tmp}}/credentials" "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" publish
   expect:
     stdout: { matches: "^http://127.0.0.1:18184/shop/runs/\\d+/\\n$" }
     stderr: "published 1 flows"
   capture: { run_url: { regex: "(http\\S+)" } }
   ```
   ```browser
   - goto: "{{priya_link}}"
   - click: Sign in to
   - goto: "{{run_url}}"
   - expect: { heading: All 1 business flows passed }
   - click: Client accepts and pays a quote from the new quote page
   - click: Open the full flow document
   - expect: { heading: Client accepts and pays a quote from the new quote page }
   - screenshot
   ```

3. **The CI workflow for acme/shop runs with publish: true**
   ```exec
   run: |
     GITHUB_ACTIONS=true ACTIONS_ID_TOKEN_REQUEST_TOKEN=fixture-oidc-request ENOCONE_CREDENTIALS_FILE="{{run.tmp}}/none" ACTIONS_ID_TOKEN_REQUEST_URL="{{env.WEB}}/oidc/token?repository=acme/shop&actor=dev" "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" publish
   expect:
     stdout: { matches: "^http://127.0.0.1:18184/shop/runs/\\d+/\\n$" }
   ```

## Then

4. **Both runs appear on the shop project page, newest first**
   ```screen
   screen: hub-project
   title: Project page
   ```
   ```http
   GET: "http://127.0.0.1:18184/api/projects/shop/runs"
   headers: { Authorization: "Bearer {{priya_token}}" }
   expect:
     status: 200
     json:
       $.runs[0].source: ci
       $.runs[0].published_by: dev via GitHub Actions
       $.runs[1].source: cli
       $.runs[1].published_by: { contains: priya+ }
   ```
   ```browser
   - goto: "http://127.0.0.1:18184/shop"
   - expect: { heading: Shop }
   - expect: "dev via GitHub Actions"
   - screenshot
   ```

5. **A token from a workflow in another repository is refused**
   ```exec
   run: |
     GITHUB_ACTIONS=true ACTIONS_ID_TOKEN_REQUEST_TOKEN=fixture-oidc-request ENOCONE_CREDENTIALS_FILE="{{run.tmp}}/none" ACTIONS_ID_TOKEN_REQUEST_URL="{{env.WEB}}/oidc/token?repository=acme/other" "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" publish
   expect:
     exit: 3
     stderr: "A workflow in acme/other can't publish to shop"
   ```
   ```exec
   run: |
     GITHUB_ACTIONS=true ACTIONS_ID_TOKEN_REQUEST_TOKEN=fixture-oidc-request ENOCONE_CREDENTIALS_FILE="{{run.tmp}}/none" ACTIONS_ID_TOKEN_REQUEST_URL="{{env.WEB}}/oidc/token?repository=acme/shop&event_name=pull_request_target" "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" publish
   expect:
     exit: 3
     stderr: "pull_request_target can't publish"
   ```
   ```exec
   run: |
     GITHUB_ACTIONS=true ACTIONS_ID_TOKEN_REQUEST_TOKEN=fixture-oidc-request ENOCONE_CREDENTIALS_FILE="{{run.tmp}}/none" ACTIONS_ID_TOKEN_REQUEST_URL="{{env.WEB}}/oidc/token?repository=acme/shop&aud_override=https://elsewhere.example" "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" publish
   expect:
     exit: 3
     stderr: "the token is for another site"
   ```

6. **An upload containing an HTML file is refused**
   ```http
   POST: "http://127.0.0.1:18184/api/projects/shop/runs"
   headers: { Authorization: "Bearer {{priya_token}}" }
   json:
     results: { schema: enocone.results/v1, run: { id: x, project: shop, env: local, driver: none, started_at: "2026-11-07T00:00:00Z", duration_ms: 0, enocone_version: "0" }, summary: { total: 0, passed: 0, failed: 0, flaky: 0, skipped: 0, steps: 0 }, flows: [] }
     files: { "assets/report.html": "PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==" }
   expect:
     status: 422
     json:
       $.error: { contains: "only screenshots" }
   ```
   ```http
   POST: "http://127.0.0.1:18184/api/projects/shop/runs"
   headers: { Authorization: "Bearer {{priya_token}}" }
   json:
     results: { schema: enocone.results/v1, run: { id: x, project: shop, env: local, driver: none, started_at: "2026-11-07T00:00:00Z", duration_ms: 0, enocone_version: "0" }, summary: { total: 0, passed: 0, failed: 0, flaky: 0, skipped: 0, steps: 0 }, flows: [] }
     files: { "assets/shot.png": "PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==" }
   expect:
     status: 422
     json:
       $.error: { contains: "not a PNG image" }
   ```

7. **A run published 31 days ago is gone, and the flows it carried are still there**
   ```exec
   run: |
     ENOCONE_DATA="{{run.tmp}}/site" "{{env.ENOCONE_BIN}}" serve --sweep --clock-offset-days 29
     ENOCONE_DATA="{{run.tmp}}/site" "{{env.ENOCONE_BIN}}" serve --sweep --clock-offset-days 31
   expect: { stdout: { matches: "^deleted 0 expired runs\\ndeleted [1-9]\\d* expired runs?\\n$" } }
   ```
   ```browser
   - goto: "{{run_url}}"
   - expect: { heading: Not found }
   - goto: "http://127.0.0.1:18184/shop"
   - expect: "No runs in the last 30 days"
   - expect: { link: Client accepts and pays a quote from the new quote page }
   ```
   ```exec
   run: |
     sh "{{env.FIXTURES}}/hub/stop.sh" 18184
   ```
