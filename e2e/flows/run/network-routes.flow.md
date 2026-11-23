---
area: Running flows
tags: [browser, network, config]
owner: enocone
priority: normal
stage: implemented
---

# A project can answer the browser's requests itself, so a suite never reaches a service it must not call

Some services a page loads cost money per call, send real messages, or
have no test environment. A suite that opens those pages calls them on
every run. A project can instead answer such requests with a file, or fail
them as a blocked host would, so the browser never sends them.

```flowchart
flowchart LR
  1[A project routes the reports API to a file and aborts the rest of the API] --> 2[A flow opens the reports page and exports]
  2 --> 3[The page shows the file's data: the first matching route answered]
  2 --> 4[The export request failed in the browser and reached no server]
  1 -.->|a route with two answers| 5[The project is refused before anything runs]
```

## Rules

- `[[run.routes]]` in `enocone.toml` lists the requests every browser
  session answers itself, each with a `url` (a Playwright URL glob: `*`
  within a path segment, `**` across them) and exactly one answer:

  ```toml
  [[run.routes]]
  url = "**/api/quarterly/*"
  fulfil = { file = "stubs/quarterly.json", content_type = "application/json" }

  [[run.routes]]
  url = "**/api/**"
  abort = true
  ```

- `fulfil` answers with the file's bytes, read when the project loads, and
  an optional `status` (200 unless given) and `content_type`. The file is
  relative to the project root.
- `abort = true` fails the request in the browser, as a blocked host would.
- The first route in the file that matches a request answers it. Requests
  no route matches go to the network as before.
- Routes apply to every browser session, signed in or not. `http` blocks
  are not routed.
- A route with no answer, with both, with an empty `url`, a status outside
  100–599 or a `fulfil` file that can't be read is refused when the project
  loads, naming the route.

## Given

1. **The demo app's reports page loads its data from the reports API, and its Export button downloads from the export API**
   ```http
   # The reports page's data endpoint answers 42 when nothing routes it
   GET: "{{env.WEB}}/api/quarterly/q1"
   expect:
     status: 200
     json:
       $.total: 42
   ```

## When

2. **A flow in a project that routes the reports API to a file and aborts the rest of the API opens the reports page and exports**
   ```exec
   # Run the routed project's flow
   run: |
     cp -R "{{env.FIXTURES}}/network-routes" "{{run.tmp}}/p"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run "{{run.tmp}}/p/flows" --out "{{run.tmp}}/out" >/dev/null 2>&1
     echo "exit $?"
   expect: { stdout: "exit 0" }
   ```

## Then

3. **The page showed the file's total, so the first matching route answered, and the data request got the file's 200**
   ```exec
   # Check the step and what the data pattern matched
   run: cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       "$.flows[0].attempts[0].steps[0].status": passed
       "$.flows[0].attempts[0].steps[0].records[3].requests[0]": "*/api/quarterly/* (count 1, status 200): 1 matched"
       "$.flows[0].attempts[0].steps[0].records[3].requests[1]": "  GET http://127.0.0.1:4010/api/quarterly/q1 200"
   ```

4. **The export request failed in the browser**
   ```exec
   # Check what the export pattern matched
   run: cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       "$.flows[0].attempts[0].steps[0].records[3].requests[2]": "*/api/export* (count 1): 1 matched"
       "$.flows[0].attempts[0].steps[0].records[3].requests[3]": "  GET http://127.0.0.1:4010/api/export/scoped/q1.csv failed"
   ```

5. **A project whose route has two answers is refused before anything runs, naming the route**
   ```exec
   # Run a project whose one route both aborts and answers with a file
   run: |
     mkdir "{{run.tmp}}/bad" && cp -R "{{run.tmp}}/p/stubs" "{{run.tmp}}/p/flows" "{{run.tmp}}/bad/"
     printf 'project = "bad"\n\n[[run.routes]]\nurl = "**/api/**"\nabort = true\nfulfil = { file = "stubs/quarterly.json" }\n' >"{{run.tmp}}/bad/enocone.toml"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/bad" run "{{run.tmp}}/bad/flows" --out "{{run.tmp}}/bad-out" 2>&1
     echo "exit $?"
   expect:
     stdout: "run.routes[0] (**/api/**): give exactly one of `abort = true` or `fulfil = { file = \"…\" }`"
   ```
