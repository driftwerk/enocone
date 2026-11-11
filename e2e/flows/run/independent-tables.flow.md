---
area: Running flows
tags: [tables, examples]
owner: enocone
priority: high
stage: draft
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

## When

2. **The flow is validated and run**

## Then

3. **Validation passes, and every row of both tables runs once and is reported under its table**

4. **A linked orders and items step keeps working as before**

5. **When a block names a column only one of the tables has, validate rejects it, naming the table that lacks it**
