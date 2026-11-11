---
area: Running flows
tags: [tables, examples, expect]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-11)
---

# A table row can set the expected status, so one table states an access matrix

An access rule is naturally a table: each kind of user, and whether they
get 200, 403 or 404 from an endpoint. Rows can already set the request and
the auth profile, but not the expected status, which is checked when the
flow is read. So one rule has to be split into a step per status, and the
reviewer loses the single table.

```flowchart
flowchart LR
  1[A table with profile and status? columns] --> 2[It runs against the demo app]
  2 --> 3[Each row checks its own status]
  2 -.->|a cell that isn't a status| 4[That row fails on its own, saying why]
```

## Rules

- An expected value in a block can come from the row: `status: "{{row.status}}"`.
  This works for every expectation that takes a single value: `status`, the
  `exit` code of a command, and the values in `json`, `headers` and
  `stdout` checks.
- The value is checked when each row runs, not when the flow is read. A
  row whose value isn't valid (`status: "forbidden"`) fails that row, with a
  message naming the value, the row and what was expected (for example,
  "a status code such as 200, or a list such as [200, 201]"). The other rows
  still run.
- A cell can hold a list (`200, 201`) where the expectation accepts one.
- `validate` still checks that every `{{row.x}}` names a column of the table.

## Given

1. **A flow has one step with a table of profiles and the status each should get from an endpoint: 200, 403 and 404**
   ```exec
   # Copy the access project and check it is valid
   run: |
     cp -R "{{env.FIXTURES}}/table-expectations" "{{run.tmp}}/p"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate "{{run.tmp}}/p/flows" 2>&1
   expect: { stdout: "2 flows OK, 0 errors" }
   ```

## When

2. **The flow runs against the demo app**
   ```exec
   # Run it and read the results
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --out "{{run.tmp}}/out" >/dev/null 2>&1
     cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       $.summary.passed: 1
       $.summary.failed: 1
   ```

## Then

3. **Each row passes with its own status, and the step passes**
   ```exec
   # Check each row of the access table, and of the exit-code table
   run: cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       "$.flows[?(@.flow.id=='access.reports')].attempts[0].steps[0].status": passed
       "$.flows[?(@.flow.id=='access.reports')].attempts[0].steps[0].rows": { len: 3 }
       "$.flows[?(@.flow.id=='access.reports')].attempts[0].steps[0].rows[1].result.records[0].status": 403
       "$.flows[?(@.flow.id=='access.reports')].attempts[0].steps[0].rows[2].result.records[0].status": 404
       "$.flows[?(@.flow.id=='access.reports')].attempts[0].steps[1].status": passed
       "$.flows[?(@.flow.id=='access.reports')].attempts[0].steps[1].rows[1].result.records[0].exit": 3
   ```

4. **A row whose status cell says "forbidden" fails on its own, saying it isn't a status, while the other rows pass**
   ```exec
   # Check the row with the typo
   run: cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       "$.flows[?(@.flow.id=='access.typo')].attempts[0].steps[0].rows[0].result.status": passed
       "$.flows[?(@.flow.id=='access.typo')].attempts[0].steps[0].rows[1].result.status": failed
       "$.flows[?(@.flow.id=='access.typo')].attempts[0].steps[0].rows[2].result.status": passed
       "$.flows[?(@.flow.id=='access.typo')].attempts[0].steps[0].error.message": { contains: "row 2 (member): http block: status \"forbidden\" isn't a status code: write one such as 200, or a list such as [200, 201]" }
   ```
