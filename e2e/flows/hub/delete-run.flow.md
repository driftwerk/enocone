---
area: Hosted site
tags: [hub, runs, delete]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-11)
---

# A project owner deletes a published run, and it is gone from the site at once

A published run normally leaves the site when it expires after 30 days.
When a run turns out to hold something it shouldn't, such as a credential
recorded before masking or personal data in a screenshot, an owner needs to
remove it straight away, without waiting and without server access.

```flowchart
flowchart LR
  1[A run holds something sensitive] --> 2[An owner deletes it, after confirming] --> 3[Its page, report and screenshots answer not found]
  2 --> 4[The Publishing page records who and when]
  6[CI deletes the run it published, through the API] --> 3
  5[A member who isn't an owner] -.->|refused| 2
```

## Rules

- The run page has a "Delete run" button for the project's owners. It asks
  for confirmation, naming the run, and says this can't be undone.
- `DELETE /api/projects/{slug}/runs/{id}` does the same. It is accepted from
  the project's owners, and from the CI workflow that published that run
  (the same repository and workflow, through GitHub's OIDC token).
- Deleting removes the run's row, its `results.json` and every screenshot
  stored for it. Its URLs then answer "not found".
- The flow documents and review comments stay: they belong to the project,
  not to one run. Comments made from that run's page keep their place on the
  flow.
- The project keeps a record of the deletion, showing who deleted which run
  and when, on its Publishing page.
- Members who aren't owners, reviewers and visitors to a public project can't
  delete runs, and the API refuses them.
- Only owners delete runs, including runs that members published.

## Given

1. **Priya owns the shop project and has published two runs; Sam is a member**
   ```exec
   # Start a test site where Priya owns shop and Sam is a member
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/start.sh" 18191 "{{run.tmp}}/site" >/dev/null
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18191 "{{run.tmp}}/site" project:shop:Shop:acme/shop "invite:shop:priya+{{run.short_id}}@example.test:owner" "invite:shop:sam+{{run.short_id}}@example.test:member" "token:priya+{{run.short_id}}@example.test" "link:priya+{{run.short_id}}@example.test" "token:sam+{{run.short_id}}@example.test" | tr '\n' ' '
   expect: { stdout: seeded }
   capture:
     priya_token: { regex: "(enc_[0-9a-f]+) http" }
     priya_link: { regex: "(http://\\S+/auth/email\\?token=\\w+)" }
     sam_token: { regex: "auth/email\\?token=\\w+ (enc_[0-9a-f]+)" }
   ```
   ```exec
   # Priya publishes the first run; the acme/shop CI workflow publishes the second
   run: |
     cp -R "{{env.FIXTURES}}/prototype" "{{run.tmp}}/p"
     printf '\n[hub]\nurl = "http://127.0.0.1:18191"\nproject = "shop"\n' >> "{{run.tmp}}/p/enocone.toml"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --prototype >/dev/null 2>&1
     env -u GITHUB_ACTIONS -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_ID_TOKEN_REQUEST_TOKEN ENOCONE_HUB_TOKEN="{{priya_token}}" "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" publish 2>/dev/null
     GITHUB_ACTIONS=true ACTIONS_ID_TOKEN_REQUEST_TOKEN=fixture-oidc-request ENOCONE_CREDENTIALS_FILE="{{run.tmp}}/none" ACTIONS_ID_TOKEN_REQUEST_URL="{{env.WEB}}/oidc/token?repository=acme/shop&actor=dev" "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" publish 2>/dev/null
     cd "{{run.tmp}}/p/enocone-report" && shot=$(ls assets/*/*.png | head -1) && echo "$shot" && ls "{{run.tmp}}/site/runs/"*/1/"$shot" >/dev/null && echo "stored"
   expect:
     stdout: { matches: "^http://127.0.0.1:18191/shop/runs/1/\\nhttp://127.0.0.1:18191/shop/runs/2/\\nassets/\\S+\\.png\\nstored\\n$" }
   capture: { shot: { regex: "(assets/\\S+\\.png)" } }
   ```

## When

2. **Priya deletes the first run from its page, after confirming**
   ```browser
   # Priya opens the first run, chooses Delete run, and confirms
   - goto: "{{priya_link}}"
   - click: Sign in to
   - goto: http://127.0.0.1:18191/shop/runs/1/
   - click: Delete run
   - expect: { text: "This can't be undone." }
   - screenshot
   - click: { role: button, name: Delete run 1 }
   - expect: { url: http://127.0.0.1:18191/shop }
   ```

## Then

3. **The run's page, report and screenshots answer not found, and the run list shows only the second run**
   ```browser
   # Priya opens the deleted run's page and one of its screenshots
   - goto: http://127.0.0.1:18191/shop/runs/1/
   - expect: { heading: Not found }
   - goto: "http://127.0.0.1:18191/shop/runs/1/{{shot}}"
   - expect: { heading: Not found }
   ```
   ```exec
   # Check the run's files are gone from the server
   run: ls -d "{{run.tmp}}/site/runs/"*/1 2>/dev/null | wc -l | tr -d ' '
   expect: { stdout: "0" }
   ```
   ```exec
   # List the project's runs
   run: 'curl -fsS -H "Authorization: Bearer {{priya_token}}" http://127.0.0.1:18191/api/projects/shop/runs'
   expect:
     json:
       $.runs: { len: 1 }
       $.runs[0].id: 2
   ```

4. **The Publishing page records that Priya deleted the run, with the time**
   ```browser
   - goto: http://127.0.0.1:18191/shop/publish
   - expect: { heading: Deleted runs }
   - expect: { text: "Run 1 deleted" }
   - expect: { text: "by priya+{{run.short_id}}@example.test on {{today}}" }
   - screenshot
   ```

5. **Sam's attempt to delete the second run through the API is refused**
   ```exec
   # Sam asks the API to delete run 2
   run: |
     curl -s -w "\n%{http_code}" -X DELETE -H "Authorization: Bearer {{sam_token}}" http://127.0.0.1:18191/api/projects/shop/runs/2
   expect:
     stdout: { matches: "Only project owners can delete runs\\.(.|\\n)*403$" }
   ```

6. **The CI workflow that published the second run deletes it through the API**
   ```exec
   # Another workflow is refused; the one that published run 2 deletes it
   run: |
     other=$(curl -fsS -H "Authorization: bearer fixture-oidc-request" "{{env.WEB}}/oidc/token?repository=acme/shop&audience=http://127.0.0.1:18191&workflow=nightly.yml" | sed 's/.*"value":"\([^"]*\)".*/\1/')
     same=$(curl -fsS -H "Authorization: bearer fixture-oidc-request" "{{env.WEB}}/oidc/token?repository=acme/shop&audience=http://127.0.0.1:18191" | sed 's/.*"value":"\([^"]*\)".*/\1/')
     curl -s -w " %{http_code}\n" -X DELETE -H "Authorization: Bearer $other" http://127.0.0.1:18191/api/projects/shop/runs/2
     curl -s -w " %{http_code}\n" -X DELETE -H "Authorization: Bearer $same" http://127.0.0.1:18191/api/projects/shop/runs/2
     curl -s -o /dev/null -w "page %{http_code}\n" -H "Authorization: Bearer {{priya_token}}" http://127.0.0.1:18191/shop/runs/2/
   expect:
     stdout: { matches: "only the runs it published(.|\\n)* 403\\n\\{\"deleted\":2\\} 200\\n" }
   ```
   ```exec
   # Stop the test site
   run: sh "{{env.FIXTURES}}/hub/stop.sh" 18191
   ```
