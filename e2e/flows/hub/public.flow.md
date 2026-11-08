---
area: Hosted site
tags: [hub, visibility, open-source]
owner: enocone
priority: normal
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-08)
---

# An owner makes an open-source project public, so anyone can read its flows and runs

Some projects are open source: their code is public, and their specs and
test runs can be too. Showing them to anyone, without an invitation, lets
users and contributors see what the product promises and whether it keeps
those promises. Other projects stay private. Every project starts private,
and an owner chooses to make it public, after being told what that shows.

```flowchart
flowchart LR
  1[A private project with runs and comments] --> 2[A visitor gets not found]
  1 --> 3[An owner makes it public, after a warning] --> 4[Anyone reads the project, runs and review pages]
  4 --> 5[Commenting and publishing still need an invitation]
  3 -.->|later| 7[Made private again: not found again]
```

## Rules

- Every project is private until an owner makes it public in the project's
  settings. It doesn't follow the GitHub repository's visibility: a public
  repository can still have runs its team would rather not publish.
- Before it becomes public, the owner is shown what that means: every run
  already published, with its screenshots, URLs and command output, becomes
  visible to anyone with the link.
- On a public project, anyone, signed in or not, can open the project page,
  its runs and reports, screenshots, and the review pages with their
  comments. Nothing on those pages can be changed by them.
- Commenting, resolving, publishing and managing members still need an
  invitation, as today. Signed-out visitors see "Sign in to comment" where
  the comment form would be, and the API refuses them.
- Public projects are reached by link; the home page doesn't list them.
- Reviewers' comments are shown to visitors on a public project: the owner
  chose to make its review public.
- For a public project, the MCP server's `review_comments` and the comments
  API work without signing in.
- A private project still answers "not found" to anyone who isn't a member,
  so its name isn't revealed.
- A public project shows a "Public" badge on its pages. An owner can make it
  private again at any time; from then on visitors get "not found".
- Public pages carry the same security headers as every other page, and
  can be found by search engines unless the owner turns that off.

## Given

1. **The shop project is private and has a published run with a comment on one flow**
   ```exec
   # Start a test site where Priya owns the private shop project and Sam reviews it
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/start.sh" 18190 "{{run.tmp}}/site" >/dev/null
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18190 "{{run.tmp}}/site" project:shop:Shop "invite:shop:priya+{{run.short_id}}@example.test:owner" "invite:shop:sam+{{run.short_id}}@example.test:reviewer" "token:priya+{{run.short_id}}@example.test" "link:sam+{{run.short_id}}@example.test"
   expect: { stdout: seeded }
   capture:
     priya_token: { regex: "(enc_[0-9a-f]+)" }
     sam_link: { regex: "(http://\\S+/auth/email\\?token=\\w+)" }
   ```
   ```exec
   # Run the draft and publish it as Priya
   run: |
     cp -R "{{env.FIXTURES}}/hub-review" "{{run.tmp}}/p"
     printf '\n[hub]\nurl = "http://127.0.0.1:18190"\nproject = "shop"\n' >> "{{run.tmp}}/p/enocone.toml"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run >/dev/null 2>&1
     env -u GITHUB_ACTIONS -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_ID_TOKEN_REQUEST_TOKEN ENOCONE_HUB_TOKEN="{{priya_token}}" "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" publish
   expect: { stdout: "/shop/runs/" }
   capture: { run_path: { regex: "(/shop/runs/\\d+/)" } }
   ```
   ```browser
   # Sam signs in and comments on the flow, then signs out
   - goto: "{{sam_link}}"
   - click: Sign in to
   - goto: "http://127.0.0.1:18190/shop/review/quotes.accept-from-email"
   - fill: { label: Your comment, value: "Should the link also work on a phone?" }
   - click: { button: Comment }
   - expect: "Open comments (1)"
   - click: Sign out
   ```

## When

2. **A signed-out visitor opens the project, its run and its review page, and gets not found each time**
   ```exec
   # Open the project, the run and the review page without signing in
   run: |
     curl -s -o /dev/null -w "%{http_code} /shop\n" "http://127.0.0.1:18190/shop"
     curl -s -o /dev/null -w "%{http_code} {{run_path}}\n" "http://127.0.0.1:18190{{run_path}}"
     curl -s -o /dev/null -w "%{http_code} /shop/review/quotes.accept-from-email\n" "http://127.0.0.1:18190/shop/review/quotes.accept-from-email"
     curl -s -o /dev/null -w "%{http_code} /nothing-here\n" "http://127.0.0.1:18190/nothing-here"
   expect: { stdout: { matches: "^404 /shop\\n404 /shop/runs/\\d+/\\n404 /shop/review/quotes.accept-from-email\\n404 /nothing-here" } }
   ```
   ```browser
   # The page a visitor sees
   - goto: "http://127.0.0.1:18190/shop"
   - expect: { heading: Not found }
   - screenshot
   ```

3. **Priya, an owner, makes the project public after reading the warning**
   ```exec
   # Get Priya a sign-in link
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18190 "{{run.tmp}}/site" "link:priya+{{run.short_id}}@example.test"
   capture: { priya_link: { regex: "(http://\\S+/auth/email\\?token=\\w+)" } }
   ```
   ```browser
   # Priya makes shop public from its Members page, after the warning
   - goto: "{{priya_link}}"
   - click: Sign in to
   - goto: "http://127.0.0.1:18190/shop/members"
   - expect: "Visibility: Private"
   - expect: "becomes visible to anyone with the link"
   - check: { label: I understand that everything published here becomes public }
   - click: { button: Make public }
   - expect: "Anyone with the link can read this project"
   ```

## Then

4. **The visitor can now read the project page, the run's report and the review page**
   ```exec
   # Open the same pages again without signing in, and check the security headers
   run: |
     curl -s -o /dev/null -w "%{http_code} /shop\n" "http://127.0.0.1:18190/shop"
     curl -s -o /dev/null -w "%{http_code} {{run_path}}\n" "http://127.0.0.1:18190{{run_path}}"
     curl -s -o /dev/null -w "%{http_code} /shop/review/quotes.accept-from-email\n" "http://127.0.0.1:18190/shop/review/quotes.accept-from-email"
     curl -s -D - -o /dev/null "http://127.0.0.1:18190/shop" | grep -ci '^content-security-policy'
   expect: { stdout: { matches: "^200 /shop\\n200 /shop/runs/\\d+/\\n200 /shop/review/quotes.accept-from-email\\n1" } }
   ```
   ```browser
   # A visitor reads the review page, with Sam's comment but not his email
   - click: Sign out
   - goto: "http://127.0.0.1:18190/shop/review/quotes.accept-from-email"
   - expect: { heading: Client accepts a quote from the email }
   - expect: "Should the link also work on a phone?"
   - expect: { text: "@example.test", hidden: true }
   - screenshot
   ```

5. **The visitor sees "Sign in to comment" instead of the form, and the API refuses their comment and their publish**
   ```browser
   # The comment form is replaced by a sign-in prompt
   - expect: "Sign in to comment"
   - expect: { button: Comment, hidden: true }
   ```
   ```exec
   # Post a comment and publish a run without signing in
   run: |
     curl -s -o /dev/null -w "comment %{http_code} %{redirect_url}\n" -H "Origin: http://127.0.0.1:18190" --data-urlencode "text=hello" "http://127.0.0.1:18190/shop/review/quotes.accept-from-email/comments"
     curl -s -o /dev/null -w "publish %{http_code}\n" -H "Content-Type: application/json" --data '{"results": {}, "files": {}}' "http://127.0.0.1:18190/api/projects/shop/runs"
   expect: { stdout: { matches: "comment 303 http://127.0.0.1:18190/login\\?next=[^\\n]*\\npublish 401" } }
   ```
   ```exec
   # The AI tool reads the comments without signing in
   run: |
     unset ENOCONE_HUB_TOKEN GITHUB_ACTIONS ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_ID_TOKEN_REQUEST_TOKEN
     ENOCONE_CREDENTIALS_FILE="{{run.tmp}}/none" python3 "{{env.FIXTURES}}/mcp/call.py" "{{env.ENOCONE_BIN}}" "{{run.tmp}}/p" review_comments '{"id": "quotes.accept-from-email"}'
   expect:
     json:
       "$.comments[0].text": "Should the link also work on a phone?"
       "$.comments[0].author": "A reviewer"
   ```

6. **The project's pages show a Public badge**
   ```browser
   # The project page shows the badge
   - goto: "http://127.0.0.1:18190/shop"
   - expect: { text: Public }
   - expect: { text: "@example.test", hidden: true }
   - screenshot
   ```

7. **When Priya makes it private again, the visitor gets not found again**
   ```exec
   # Get Priya a new sign-in link
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18190 "{{run.tmp}}/site" "link:priya+{{run.short_id}}@example.test"
   capture: { priya_link2: { regex: "(http://\\S+/auth/email\\?token=\\w+)" } }
   ```
   ```browser
   # Priya makes shop private again
   - goto: "{{priya_link2}}"
   - click: Sign in to
   - goto: "http://127.0.0.1:18190/shop/members"
   - click: { button: Make private }
   - expect: "Visibility: Private"
   - click: Sign out
   ```
   ```exec
   # Open the pages without signing in once more, then stop the test site
   run: |
     curl -s -o /dev/null -w "%{http_code} /shop\n" "http://127.0.0.1:18190/shop"
     curl -s -o /dev/null -w "%{http_code} /shop/review/quotes.accept-from-email\n" "http://127.0.0.1:18190/shop/review/quotes.accept-from-email"
     sh "{{env.FIXTURES}}/hub/stop.sh" 18190
   expect: { stdout: { matches: "^404 /shop\\n404 /shop/review/quotes.accept-from-email" } }
   ```
