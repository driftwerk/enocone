---
area: Running flows
tags: [tables, examples]
owner: enocone
priority: high
stage: draft
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
   json: { customer: "{{row.customer}}", total: {{row.total}} }
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
json: { customer: "{{row.customer}}", items: {{row.items}} }
expect:
  json:
    $.total: "{{row.total}}"
```

## Given

1. **Priya's flow has a discount table with four rows, one of them wrong**

## When

2. **She runs the flow**

## Then

3. **Each row is checked on its own: three pass and the wrong one fails, naming the row and the value that came back**

4. **The report shows the table with each row's result**

5. **An order table linked to its line items sends each order with its items, and the totals match**

6. **A block that names a column the table doesn't have is rejected before anything runs, with the line to fix**

7. **Sam comments on one row on the team site, and the comment stays on that row after another row changes**
