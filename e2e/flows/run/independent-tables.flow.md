---
area: Running flows
tags: [tables, examples]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-11)
---

# Several tables in one step are separate lists of cases unless one links to another

A step can hold more than one table: one of callers who are allowed and one
of callers who are refused, for example. Today every extra table is treated
as linked to the first, so such a step fails validation, and the only fix
is a step per table. A table should be linked only when it says so, by
having a column named after another table.

```flowchart
flowchart LR
  1[A step with two tables, no link column] --> 2[Validated and run] --> 3[Every row of both tables runs once]
  4[A second table with a column named after the first] --> a[Linked, as today]
  2 -.->|a block names a column one table lacks| 5[Rejected, naming the table]
```

## Rules

- Tables in one step are independent unless one is linked: a table is
  linked when it has a column named after another table in the step
  (`Table: orders`, and an `orders` column in `items`). This is the same
  link as today.
- With independent tables, the step's blocks run once for each row of each
  table, in order: the first table's rows, then the next table's.
- Every `{{row.x}}` in the blocks must name a column of each independent
  table. If one table lacks it, `validate` names the table and the column.
- Independent tables don't need names. When they have them, the report and
  results show the table's name beside each row.
- The report draws each table with its rows' results. Comments on the team
  site can be left on any row of any table.
- `validate`'s errors about tables say which rule applied: "linked to
  `orders` through its `orders` column", or "independent".

## Given

1. **A step has a table of allowed callers and a separate table of refused callers, with the same columns**
   ```exec
   # Copy the callers project
   run: cp -R "{{env.FIXTURES}}/independent-tables" "{{run.tmp}}/p"
   ```

## When

2. **The flow is validated and run**
   ```exec
   # Validate the flow, run it, and read the results
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate "{{run.tmp}}/p/flows" 2>&1
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --out "{{run.tmp}}/out" >/dev/null 2>&1
     echo "exit $?"
   expect: { stdout: { matches: "1 flows OK, 0 errors(.|\\n)*exit 0" } }
   ```

## Then

3. **Validation passes, and every row of both tables runs once and is reported under its table**
   ```exec
   # Check each row ran once, under its table
   run: cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       "$.flows[?(@.flow.id=='access.callers')].attempts[0].steps[0].status": passed
       "$.flows[?(@.flow.id=='access.callers')].attempts[0].steps[0].rows": { len: 3 }
       "$.flows[?(@.flow.id=='access.callers')].attempts[0].steps[0].rows[0].table_name": allowed
       "$.flows[?(@.flow.id=='access.callers')].attempts[0].steps[0].rows[0].label": admin
       "$.flows[?(@.flow.id=='access.callers')].attempts[0].steps[0].rows[1].table_name": refused
       "$.flows[?(@.flow.id=='access.callers')].attempts[0].steps[0].rows[1].label": member
       "$.flows[?(@.flow.id=='access.callers')].attempts[0].steps[0].rows[1].result.records[0].status": 403
       "$.flows[?(@.flow.id=='access.callers')].attempts[0].steps[0].rows[2].label": stranger
       "$.flows[?(@.flow.id=='access.callers')].attempts[0].steps[0].rows[2].result.records[0].status": 404
   ```
   ```browser
   # The report draws both tables with their rows' results
   - goto: "file://{{run.tmp}}/out/index.html#flow=access.callers"
   - expect: { text: "Table: allowed" }
   - expect: { text: "Table: refused" }
   - expect: { role: row, name: "admin 200 passed" }
   - expect: { role: row, name: "stranger 404 passed" }
   - screenshot
   ```

4. **A linked orders and items step keeps working as before**
   ```exec
   # Run the linked orders and items flow
   run: |
     cp -R "{{env.FIXTURES}}/tables" "{{run.tmp}}/orders"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/orders" run --out "{{run.tmp}}/orders-out" >/dev/null 2>&1
     cat "{{run.tmp}}/orders-out/results.json"
   expect:
     json:
       "$.flows[?(@.flow.id=='pricing.orders')].attempts[0].steps[0].status": passed
       "$.flows[?(@.flow.id=='pricing.orders')].attempts[0].steps[0].rows": { len: 2 }
       "$.flows[?(@.flow.id=='pricing.orders')].attempts[0].steps[0].rows[0].label": "A, new"
       "$.flows[?(@.flow.id=='pricing.orders')].attempts[0].steps[0].rows[0].result.records[0].request_body": { contains: "book" }
   ```

5. **When a block names a column only one of the tables has, validate rejects it, naming the table that lacks it**
   ```exec
   # Validate a flow whose refused table has `caller` instead of `who`
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate "{{run.tmp}}/p/bad" 2>&1
   expect:
     exit: 2
     stdout: { matches: "missing-column\\.flow\\.md:24: error: .*table `refused` has no column `who` \\(columns: caller, status\\)\\. The step's tables are independent" }
   ```
