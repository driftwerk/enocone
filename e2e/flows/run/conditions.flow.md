---
area: Running flows
tags: [conditions, tables, capture]
owner: enocone
priority: normal
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-11)
---

# A step or a table row can run only when a condition holds

Setup flows often depend on what they find. For example, a flow should
invite a user only if their status is still Pending. Today the flow has to
be split and started once per case from CI. A condition on a step, checked
against values captured earlier and against the row, lets one flow handle
every case and shows the reviewer why a step was skipped.

```flowchart
flowchart LR
  1[A step captures each user's status] --> 2[The invite step runs only if status is Pending]
  2 --> 3[Pending rows run, other rows are skipped with the condition]
  4[A step whose condition is false] --> a[Skipped, and later steps still run]
  5[A condition enocone can't read] -.->|rejected by validate| b[With its line]
```

## Rules

- A step can have a line `Only if: <condition>`, directly under its heading,
  next to its tables and blocks. For example:
  `Only if: {{row.status}} == "Pending"`.
- The condition is checked when the step would run, and again for each row
  of its table. When it's false, the step (or the row) is skipped and
  reported as skipped, with the condition and the values it saw: `status
  was "Active"`.
- A condition compares two values with `==`, `!=`, `<`, `<=`, `>`, `>=`, or
  checks membership with `in [ … ]`. Conditions combine with `and`, `or`,
  `not`, and parentheses. Values are `{{…}}` templates (captures, `row`,
  `env`, `vars`), quoted strings, numbers and `true`/`false`.
- A condition that doesn't parse, or names a column the table doesn't
  have, is a `validate` error with the line.
- A skipped step doesn't fail the flow. Later steps still run.
- The line reads `Only if:`, not `when:`, because the flow's sections are
  already called "When".

## Given

1. **A flow captures the status of three users: two Pending and one Active**
   ```exec
   # Copy the invites project and check it is valid
   run: |
     cp -R "{{env.FIXTURES}}/conditions" "{{run.tmp}}/p"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate "{{run.tmp}}/p/flows" 2>&1
   expect: { stdout: "1 flows OK, 0 errors" }
   ```

## When

2. **Its invite step, with a table of the three users, runs only if the row's status is Pending**
   ```exec
   # Run the invites flow and read the results
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --out "{{run.tmp}}/out" >/dev/null 2>&1
     cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       $.summary.passed: 1
       $.summary.failed: 0
   ```

## Then

3. **The two Pending rows run, and the Active row is skipped, showing the condition and the status it saw**
   ```exec
   # Check each row: Ana and Cy invited, Ben skipped with what was seen
   run: cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       "$.flows[?(@.flow.id=='invites.pending')].attempts[0].steps[1].status": passed
       "$.flows[?(@.flow.id=='invites.pending')].attempts[0].steps[1].rows[0].result.status": passed
       "$.flows[?(@.flow.id=='invites.pending')].attempts[0].steps[1].rows[1].result.status": skipped
       "$.flows[?(@.flow.id=='invites.pending')].attempts[0].steps[1].rows[1].result.note": { matches: "^skipped: only if \\{\\{row\\.status\\}\\} == \"Pending\": status was \"Active\"$" }
       "$.flows[?(@.flow.id=='invites.pending')].attempts[0].steps[1].rows[2].result.status": passed
       "$.flows[?(@.flow.id=='invites.pending')].attempts[0].steps[3].status": passed
   ```
   ```browser
   # The report shows Ben's row as skipped, and why
   - goto: "file://{{run.tmp}}/out/index.html#flow=invites.pending"
   - expect: { text: "== \"Pending\"" }
   - expect: { role: row, name: "ben Active skipped" }
   - click: { role: row, name: "ben Active skipped" }
   - expect: { text: "status was \"Active\"" }
   - screenshot
   ```

4. **A step whose condition is false is skipped, and the steps after it still run**
   ```exec
   # Check the reminder step was skipped and the last step ran
   run: cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       "$.flows[?(@.flow.id=='invites.pending')].attempts[0].steps[2].status": skipped
       "$.flows[?(@.flow.id=='invites.pending')].attempts[0].steps[2].note": { contains: "ben_status was \"Active\"" }
       "$.flows[?(@.flow.id=='invites.pending')].attempts[0].steps[2].records": { exists: false }
       "$.flows[?(@.flow.id=='invites.pending')].attempts[0].steps[3].status": passed
       "$.flows[?(@.flow.id=='invites.pending')].attempts[0].steps[3].records[0].response_body": { not: { contains: "ben" } }
   ```

5. **A condition that can't be read is rejected by validate, with its line**
   ```exec
   # Validate a flow with `=` for `==`, and one naming a missing column
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate "{{run.tmp}}/p/bad" 2>&1
   expect:
     exit: 2
     stdout: { matches: "unreadable\\.flow\\.md:14: error: `Only if: .*` can't be read: use `==` to compare, not `=`(.|\\n)*unreadable\\.flow\\.md:27: error: .*the table has no column `state`" }
   ```
