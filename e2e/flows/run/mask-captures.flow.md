---
area: Running flows
tags: [secrets, capture, masking]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-11)
---

# A captured value marked secret is masked everywhere the run records it

Some captured values are credentials: a link from an invite or password-reset
email lets anyone who holds it act as that person until it expires. Today
only `{{secret.*}}` values are masked, so such a link ends up readable in
`results.json`, the report, page snapshots, console lines and everything
published to the team site. Many teams want those values in full while
debugging, so masking a capture is something a flow or project asks for.

```flowchart
flowchart LR
  1[A flow captures an invite link marked secret and fails on its page] --> 2[It runs and is published]
  2 --> 3[The link is masked in results, report and console]
  2 --> 5[Snapshot and screenshots are still recorded]
  2 --> 6[The published run doesn't contain it]
  7[The same flow without secret: true] -.->|unchanged| a[Recorded in full, as today]
```

## Rules

- A capture can be marked secret: `capture: { invite_link: { link: "Accept", secret: true } }`
  (any capture kind: `json`, `regex`, `link`, `header`). Default off.
- From the moment it is captured, its value is masked like a `{{secret.*}}`
  value wherever the run records it:
  - captures
  - action descriptions (`goto …`)
  - request and response records
  - page snapshots and console lines
  - error messages
  - `results.json`, `index.html`, `junit.xml`, `summary.md`
  - the payload published to the team site

  Screenshots are images and are kept as they are.
- Its URL-encoded form (`%3A%2F%2F…`) is masked too, and so is the value of
  any query-string parameter that holds it, because a link's token often
  appears inside another URL.
- A project can mark captures secret by name for every flow:
  `[mask] captures = ["invite_link", "reset_link"]` in `enocone.toml`.
  Off by default.
- Masking only changes what is recorded. The flow still uses the real value.
- Without either setting, captured values are recorded in full, exactly as
  today.
- A masked value shows which capture it was: `•••invite_link•••`, so
  someone debugging a failed run can tell which value went where.

## Given

1. **A flow captures an invite link from an email, marked secret, opens it, and fails on the page**
   ```exec
   # Copy the invites project, and start a test site to publish to
   run: |
     cp -R "{{env.FIXTURES}}/mask" "{{run.tmp}}/p"
     grep -c "secret: true" "{{run.tmp}}/p/flows/invite.flow.md"
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/start.sh" 18192 "{{run.tmp}}/site" >/dev/null
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18192 "{{run.tmp}}/site" project:shop:Shop "token:owner+{{run.short_id}}@example.test" | tr '\n' ' '
   expect: { stdout: { matches: "^1\\n.*seeded" } }
   capture: { owner_token: { regex: "(enc_[0-9a-f]+)", secret: true } }
   ```

## When

2. **The flow runs, and its run is published to the team site**
   ```exec
   # Run the flow, keeping the console output, then publish the run
   run: |
     printf '\n[hub]\nurl = "http://127.0.0.1:18192"\nproject = "shop"\n' >> "{{run.tmp}}/p/enocone.toml"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run "{{run.tmp}}/p/flows" --out "{{run.tmp}}/p/out" > "{{run.tmp}}/p/console.txt" 2>&1
     echo "exit $?"
     env -u GITHUB_ACTIONS -u ACTIONS_ID_TOKEN_REQUEST_URL ENOCONE_HUB_TOKEN="{{owner_token}}" "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" publish "{{run.tmp}}/p/out/results.json" 2>/dev/null
   expect:
     stdout: { matches: "^exit 1\\nhttp://127.0.0.1:18192/shop/runs/1/\\n$" }
   ```

## Then

3. **The link appears nowhere in results.json, the report, junit.xml, summary.md or the console output**
   ```exec
   # Count the link, raw and URL-encoded, in each output (counts only)
   run: |
     leaks="$PWD/{{env.FIXTURES}}/mask/leaks.sh"; case "{{env.FIXTURES}}" in /*) leaks="{{env.FIXTURES}}/mask/leaks.sh";; esac
     cd "{{run.tmp}}/p" && sh "$leaks" out/results.json out/index.html out/junit.xml out/summary.md console.txt
   expect:
     stdout: { matches: "^results\\.json raw=0 encoded=0 masked=[1-9]\\d*\\nindex\\.html raw=0 encoded=0 masked=[1-9]\\d*\\njunit\\.xml raw=0 encoded=0 masked=\\d+\\nsummary\\.md raw=0 encoded=0 masked=\\d+\\nconsole\\.txt raw=0 encoded=0 masked=\\d+\\n$" }
   ```

4. **Its URL-encoded form, inside another link on the page, is masked too**
   ```exec
   # Check the page snapshot: the sign-in link's next= holds the masked link
   run: cat "{{run.tmp}}/p/out/results.json"
   expect:
     json:
       "$.flows[0].attempts[0].steps[1].snapshot": { contains: "/login?next=•••invite_link•••" }
       "$.flows[0].attempts[0].steps[1].records[0].description": "open •••invite_link•••"
   ```

5. **The failed step still has its page snapshot and screenshot, with the link masked in the snapshot**
   ```exec
   # Check the failed step kept its snapshot and screenshot
   run: |
     cat "{{run.tmp}}/p/out/results.json"
   expect:
     json:
       "$.flows[0].attempts[0].steps[1].status": failed
       "$.flows[0].attempts[0].steps[1].snapshot": { contains: "Your conveyancing quote" }
       "$.flows[0].attempts[0].steps[1].screenshot": { matches: "\\.png$" }
   ```
   ```exec
   # The screenshot file is there
   run: ls "{{run.tmp}}/p/out/assets/invite/" | grep -c png
   expect: { stdout: "1" }
   ```

6. **The run as published to the team site doesn't contain the link either**
   ```exec
   # Count the link in what the site stored for the run
   run: |
     sh "{{env.FIXTURES}}/mask/leaks.sh" "{{run.tmp}}/site/runs/"*/1/results.json
     sh "{{env.FIXTURES}}/hub/stop.sh" 18192 >/dev/null
   expect:
     stdout: { matches: "^results\\.json raw=0 encoded=0 masked=[1-9]\\d*\\n$" }
   ```

7. **The same flow without `secret: true` records the link in full**
   ```exec
   # Run the flow without secret: true, and count the link
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run "{{run.tmp}}/p/plain" --out "{{run.tmp}}/p/plain-out" >/dev/null 2>&1
     sh "{{env.FIXTURES}}/mask/leaks.sh" "{{run.tmp}}/p/plain-out/results.json"
   expect:
     stdout: { matches: "^results\\.json raw=[1-9]\\d* encoded=[1-9]\\d* masked=0\\n$" }
   ```

8. **With `[mask] captures = ["invite_link"]` in enocone.toml and no `secret: true`, the link is masked**
   ```exec
   # Mask invite_link for the whole project, run the plain flow again, and count
   run: |
     printf '\n[mask]\ncaptures = ["invite_link"]\n' >> "{{run.tmp}}/p/enocone.toml"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run "{{run.tmp}}/p/plain" --out "{{run.tmp}}/p/masked-out" >/dev/null 2>&1
     sh "{{env.FIXTURES}}/mask/leaks.sh" "{{run.tmp}}/p/masked-out/results.json"
   expect:
     stdout: { matches: "^results\\.json raw=0 encoded=0 masked=[1-9]\\d*\\n$" }
   ```
