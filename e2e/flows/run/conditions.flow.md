---
area: Running flows
tags: [conditions, tables, capture]
owner: enocone
priority: normal
stage: draft
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

## Open questions

- Is `Only if:` the right wording? I recommend it over `when:` because the
  flow's own sections are called "When", and a reviewer reads "Only if"
  as plain English.

## Given

1. **A flow captures the status of three users: two Pending and one Active**

## When

2. **Its invite step, with a table of the three users, runs only if the row's status is Pending**

## Then

3. **The two Pending rows run, and the Active row is skipped, showing the condition and the status it saw**

4. **A step whose condition is false is skipped, and the steps after it still run**

5. **A condition that can't be read is rejected by validate, with its line**
