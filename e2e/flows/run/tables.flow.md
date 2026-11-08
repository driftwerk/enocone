---
area: Running flows
tags: [tables, examples]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-08)
---

# A rule written as a table of examples runs once per row, and linked tables describe data with parts

Many business rules are really tables: prices, discounts, who may do what,
how long something lasts. Written as a sentence, a rule shows one case; as a
table it shows them all, and a reviewer spots the missing row or the wrong
number at a glance. A flow can hold a plain Markdown table of examples, one
row per case, with a step that runs once for each row and marks each row
passed or failed.

Some examples need data with parts: an order and its line items. Rather than
tables inside cells, a second table holds the parts and links each one to
its row in the first by an id, so both stay flat and readable on GitHub, on
the team site and in the report.

```flowchart
flowchart LR
  1[A step holds a table of examples] --> 2[Its block runs once per row] --> 3[Each row passes or fails on its own] --> 4[The report shows the table, row by row]
  5[A linked table adds parts to each row] --> 2
  4 --> 7[Reviewers comment on a single row]
  1 -.->|the block names a column the table lacks| 6[Rejected before anything runs, with the line]
```

## Rules

- A table is an ordinary Markdown table placed in a step. The step's blocks
  run once per row; `{{row.<column>}}` is that row's value in a column, with
  the column's header written in lower case and spaces as `_`.
- A column whose header ends in `?` (`discount?`) holds the expected result;
  the others are inputs. Blocks read it without the `?` (`{{row.discount}}`).
- Tables run only in steps. A table anywhere else in the flow, such as under
  Rules, is shown as part of the document but not run.
- Each row is its own result: passed, failed (with what was expected and
  what came back) or skipped. The step passes only when every row does.
- The report shows the table with each row coloured by its result, and the
  failing cells marked. Results, JUnit and the published run list rows
  individually, so a failure names the row ("row 3: returning, 120").
- A table can be named with a caption line just above it
  (`Table: orders`). A second table links to it with a column whose header
  names the first table (`orders`), holding the id of the row it belongs to;
  the first table's id column is its first column.
- In a block, `{{row.<table>}}` is the list of linked rows that belong to the
  current row, so a request can send an order with all its items. A linked
  row whose id doesn't exist in the first table is an error in the flow,
  reported with its line.
- Linking goes one level deep: orders → items. A linked table can't itself
  be linked to (items → options) in this version.
- A `{{row.…}}` naming a column that isn't in the table is an error in the
  flow, reported before anything runs.
- Rows run one after another, each with a fresh `{{row}}`; captures don't
  carry from one row to the next.
- Tables are best checked through `http` or `exec` blocks. Browser blocks
  work too, but each row drives the browser, which is slow.
- On enoc.one, a reviewer can comment on one row of a table, and the comment
  stays on that row when other rows change.
- The MCP server's guide teaches tables, so the AI tool writes a rule with
  many cases as a table rather than as many steps.

## How it reads

A step with a table of examples, as it's written in the flow. The table is
what people review; the block under it says how one row is checked, using
that row's values:

````markdown
2. **The discount depends on the customer and the order total**

   | customer  | total | discount? |
   |-----------|------:|----------:|
   | new       |    40 |        0% |
   | new       |   120 |        5% |
   | returning |    40 |        5% |
   | returning |   120 |       10% |

   ```http
   POST: "{{env.API}}/quotes"
   json: { customer: "{{row.customer}}", total: "{{row.total}}" }
   expect:
     json:
       $.discount: "{{row.discount}}"
   ```
````

After a run, the report shows the same table with each row's result. If the
app gave returning customers 5% on large orders, the last row would fail and
say what came back:

| customer  | total | discount? | result                 |
|-----------|------:|----------:|------------------------|
| new       |    40 |        0% | ✓ passed               |
| new       |   120 |        5% | ✓ passed               |
| returning |    40 |        5% | ✓ passed               |
| returning |   120 |       10% | ✗ failed: got 5%       |

Linked tables, for data with parts. The first table is named `orders`; the
second links each line item to its order through its `orders` column:

Table: orders

| order | customer  | total? |
|-------|-----------|-------:|
| A     | new       |  52.50 |
| B     | returning |  20.00 |

Table: items

| orders | item | qty | price |
|--------|------|----:|------:|
| A      | book |   1 | 40.00 |
| A      | pen  |   5 |  2.50 |
| B      | pen  |   8 |  2.50 |

The step runs once per order. `{{row.items}}` is that order's items, so the
request sends order A with its book and five pens, and checks the total
against the `total?` column:

```http
POST: "{{env.API}}/orders"
json: { customer: "{{row.customer}}", items: "{{row.items}}" }
expect:
  json:
    $.total: "{{row.total}}"
```

## Given

1. **Priya's flow has a discount table with four rows, one of them wrong**
   ```exec
   # Copy Priya's pricing project and check it is valid
   run: |
     cp -R "{{env.FIXTURES}}/tables" "{{run.tmp}}/p"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate "{{run.tmp}}/p/flows" 2>&1
   expect: { stdout: "2 flows OK, 0 errors" }
   ```

## When

2. **She runs the flow**
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

3. **Each row is checked on its own: three pass and the wrong one fails, naming the row and the value that came back**
   ```exec
   # Check each row's result
   run: |
     cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       "$.flows[?(@.flow.id=='pricing.discount')].attempts[0].steps[0].rows": { len: 4 }
       "$.flows[?(@.flow.id=='pricing.discount')].attempts[0].steps[0].rows[0].result.status": passed
       "$.flows[?(@.flow.id=='pricing.discount')].attempts[0].steps[0].rows[1].result.status": passed
       "$.flows[?(@.flow.id=='pricing.discount')].attempts[0].steps[0].rows[2].result.status": passed
       "$.flows[?(@.flow.id=='pricing.discount')].attempts[0].steps[0].rows[3].result.status": failed
       "$.flows[?(@.flow.id=='pricing.discount')].attempts[0].steps[0].status": failed
       "$.flows[?(@.flow.id=='pricing.discount')].attempts[0].steps[0].error.message": { contains: "row 4 (returning, 120): $.discount: expected \"15%\", got \"10%\"" }
   ```

4. **The report shows the table with each row's result**
   ```browser
   - goto: "file://{{run.tmp}}/out/index.html#flow=pricing.discount"
   - expect: { role: columnheader, name: Result }
   - expect: { role: columnheader, name: "discount?" }
   - expect: { text: "got 10%" }
   - expect: { role: row, name: "returning 120 15% got 10% failed" }
   - screenshot
   ```

5. **An order table linked to its line items sends each order with its items, and the totals match**
   ```exec
   # Check the orders and their items
   run: |
     cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       "$.flows[?(@.flow.id=='pricing.orders')].attempts[0].steps[0].status": passed
       "$.flows[?(@.flow.id=='pricing.orders')].attempts[0].steps[0].rows": { len: 2 }
       "$.flows[?(@.flow.id=='pricing.orders')].attempts[0].steps[0].rows[0].label": "A, new"
       "$.flows[?(@.flow.id=='pricing.orders')].attempts[0].steps[0].rows[0].result.records[0].request_body": { contains: "book" }
       "$.flows[?(@.flow.id=='pricing.orders')].attempts[0].steps[0].rows[1].result.records[0].request_body": { not: { contains: "book" } }
       "$.flows[?(@.flow.id=='pricing.orders')].attempts[0].steps[0].rows[1].result.status": passed
   ```

6. **A block that names a column the table doesn't have is rejected before anything runs, with the line to fix**
   ```exec
   # Validate a flow that uses an unknown column
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate "{{run.tmp}}/p/bad" 2>&1
   expect:
     exit: [2]
     stdout: { matches: "bad/unknown-column\\.flow\\.md:17: error: `.*row\\.totl.*`: the table has no column `totl`" }
   ```

7. **Sam comments on one row on the team site, and the comment stays on that row after another row changes**
   ```exec
   # Start a test site with Priya and Sam on the shop project
   run: |
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/start.sh" 18189 "{{run.tmp}}/site" >/dev/null
     BIN="{{env.ENOCONE_BIN}}" WEB="{{env.WEB}}" OWNER="owner+{{run.short_id}}@example.test" sh "{{env.FIXTURES}}/hub/seed.sh" 18189 "{{run.tmp}}/site" project:shop:Shop "invite:shop:priya+{{run.short_id}}@example.test:member" "invite:shop:sam+{{run.short_id}}@example.test:reviewer" "token:priya+{{run.short_id}}@example.test" "link:sam+{{run.short_id}}@example.test"
   expect: { stdout: seeded }
   capture:
     priya_token: { regex: "(enc_[0-9a-f]+)" }
     sam_link: { regex: "(http://\\S+/auth/email\\?token=\\w+)" }
   ```
   ```exec
   # Publish the run to the test site
   run: |
     printf '\n[hub]\nurl = "http://127.0.0.1:18189"\nproject = "shop"\n' >> "{{run.tmp}}/p/enocone.toml"
     env -u GITHUB_ACTIONS -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_ID_TOKEN_REQUEST_TOKEN ENOCONE_HUB_TOKEN="{{priya_token}}" "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" publish "{{run.tmp}}/out/results.json"
   expect: { stdout: "/shop/runs/" }
   ```
   ```browser
   # Sam comments on the row "returning, 120"
   - goto: "{{sam_link}}"
   - click: Sign in to
   - goto: "http://127.0.0.1:18189/shop/review/pricing.discount"
   - expect: { role: cell, name: "15%" }
   - select: { label: Comment on, option: "Step 1, row: returning, 120" }
   - fill: { label: Your comment, value: "Returning customers get 10% from 100; 15% was the old rate." }
   - click: { button: Comment }
   - expect: "Step 1, row: returning, 120"
   ```
   ```exec
   # Change another row (new, 40 → 50), run and publish again
   run: |
     sed -i.bak 's/| new       |    40 |        0% |/| new       |    50 |        0% |/' "{{run.tmp}}/p/flows/pricing/discount.flow.md"
     grep -c "|    50 |" "{{run.tmp}}/p/flows/pricing/discount.flow.md"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --out "{{run.tmp}}/out2" >/dev/null 2>&1
     env -u GITHUB_ACTIONS -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_ID_TOKEN_REQUEST_TOKEN ENOCONE_HUB_TOKEN="{{priya_token}}" "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" publish "{{run.tmp}}/out2/results.json"
   expect: { stdout: { matches: "^1\\n(?s).*/shop/runs/" } }
   ```
   ```browser
   # The comment is still on its row, not outdated
   - goto: "http://127.0.0.1:18189/shop/review/pricing.discount"
   - expect: { role: cell, name: "50" }
   - expect: "15% was the old rate"
   - expect: "Step 1, row: returning, 120"
   - expect: { text: Outdated, hidden: true }
   - screenshot
   ```
   ```exec
   # Stop the test site
   run: |
     sh "{{env.FIXTURES}}/hub/stop.sh" 18189
   ```
