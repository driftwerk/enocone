---
area: Running flows
tags: [browser, network, expect]
owner: enocone
priority: normal
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-12)
---

# A browser step can check the requests the page made

Some behaviour shows only in the requests a page makes: "the export
downloads only from the scoped endpoints", or "this screen never calls the
admin API". A flow can't check that today. A browser step that checks the
page's requests can, without recording their bodies.

```flowchart
flowchart LR
  1[A reports page that loads its data and has an export button] --> 2[A flow opens it, exports, and checks the requests]
  2 --> 3[The data request is counted with status 200, and no admin request was made]
  2 --> 4[Every export request went to the scoped endpoint]
  2 -.->|a request the page never made| 5[Fails, listing the requests it did see]
  2 --> 6[The report lists the matched requests, without bodies or headers]
```

## Rules

- A browser block can check the page's requests with a `requests:` action,
  a list of patterns, each with what to expect:

  ```yaml
  - requests:
      - url: "*/api/reports/*"
        expect: { count: ">=1", status: 200 }
      - url: "*/api/admin/*"
        expect: { count: 0 }
      - url: "*/api/export*"
        method: GET
        expect: { urls: "*/api/export/scoped/*" }
  ```

- A pattern matches by `url`, where `*` stands for any characters, and
  optionally by `method`.
- `expect` can check:
  - `count`: a number, or a comparison such as `">=1"` or `"<3"`;
  - `status`: a status code, or a list of them, that every matching
    response must have;
  - `urls`: a pattern that every matching request's URL must match.
- The check covers the requests the page made since the step started, up
  to the `requests:` action. Before checking, it waits for requests still
  in flight to finish (up to the step's timeout), so a request a click
  just started is counted.
- `count: 0` passes only if no matching request was made.
- A failure names the pattern, what was expected, and the matching
  requests it saw (up to 10, with method, URL and status).
- The report lists the requests each pattern matched, with method, URL and
  status. Request and response bodies and headers aren't recorded. URLs are
  masked like other secrets.

## Given

1. **The demo app has a reports page that loads its data from the reports API, never calls the admin API, and has an export button that downloads from a scoped endpoint**
   ```http
   # The reports page's data endpoint answers
   GET: "{{env.WEB}}/api/quarterly/q1"
   expect:
     status: 200
     json:
       $.total: 42
   ```

## When

2. **A flow opens the reports page, clicks Export, and checks the requests the page made**
   ```exec
   # Run a flow that opens the reports page, exports, and checks its requests
   run: |
     cp -R "{{env.FIXTURES}}/network-requests" "{{run.tmp}}/p"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run "{{run.tmp}}/p/flows" --out "{{run.tmp}}/out" >/dev/null 2>&1
     echo "exit $?"
   expect: { stdout: "exit 0" }
   ```

## Then

3. **The reports request was made with status 200, and no admin request was made**
   ```exec
   # Check what the data and admin patterns matched
   run: cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       "$.flows[0].attempts[0].steps[0].status": passed
       "$.flows[0].attempts[0].steps[0].records[4].requests[0]": "*/api/quarterly/* (count >=1, status 200): 1 matched"
       "$.flows[0].attempts[0].steps[0].records[4].requests[1]": "  GET http://127.0.0.1:4010/api/quarterly/q1 200"
       "$.flows[0].attempts[0].steps[0].records[4].requests[2]": "*/api/admin/* (count 0): 0 matched"
   ```

4. **Every export request went to the scoped endpoint**
   ```exec
   # Check what the export pattern matched
   run: cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       "$.flows[0].attempts[0].steps[0].records[4].requests[3]": "GET */api/export* (count >=1, all to */api/export/scoped/*): 1 matched"
       "$.flows[0].attempts[0].steps[0].records[4].requests[4]": "  GET http://127.0.0.1:4010/api/export/scoped/q1.csv 200"
   ```

5. **A check expecting a request the page never made fails, listing the requests it did see**
   ```exec
   # Run a flow that expects a request to an audit API the page never calls
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run "{{run.tmp}}/p/bad" --out "{{run.tmp}}/bad-out" >/dev/null 2>&1
     cat "{{run.tmp}}/bad-out/results.json"
   expect:
     json:
       "$.flows[0].attempts[0].steps[0].status": failed
       "$.flows[0].attempts[0].steps[0].error.message": "requests */api/audit/*: expected >=1 matching, saw 0\n  requests the page made (2):\n    GET http://127.0.0.1:4010/reports 200\n    GET http://127.0.0.1:4010/api/quarterly/q1 200"
   ```

6. **The report lists the requests each pattern matched, with method, URL and status, and no bodies or headers**
   ```exec
   # The export's body and the page's headers appear nowhere in the outputs (counts only)
   run: |
     cd "{{run.tmp}}/out" && grep -c -e "north,20" -e "text/csv" results.json index.html junit.xml summary.md | tr '\n' ' '
   expect:
     stdout: "results.json:0 index.html:0 junit.xml:0 summary.md:0 "
   ```
   ```exec
   # The report page carries the matched requests
   run: grep -c "api/export/scoped/q1.csv 200" "{{run.tmp}}/out/index.html"
   expect: { stdout: "1" }
   ```
