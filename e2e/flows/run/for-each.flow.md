---
area: Running flows
tags: [loops, capture, setup]
owner: enocone
priority: normal
stage: draft
---

# A step can run once for each item of a list captured earlier

A setup flow often needs to tidy up before it starts, for example by
deleting every old resource with a given name before creating a fresh one.
The flow can capture the ids, but it can't act on each of them, so today
it falls back to a command with curl and jq. That loses enocone's sign-in
profiles, masking and per-request report. A step that repeats over a
captured list keeps all of that.

```flowchart
flowchart LR
  1[Three demo resources and one to keep] --> 2[The flow captures the demo ids and deletes each one]
  2 --> 3[Three deletes, each reported as a row]
  2 -.->|nothing matched| 4[The step is skipped, saying the list was empty]
  2 -.->|more items than the limit| 5[The step fails before running any]
```

## Rules

- A step can have a line `For each: {{old_ids}}` under its heading. Its
  blocks run once per item, with the item as `{{item}}`. When items are
  objects, their fields are `{{item.name}}` and so on.
- The list comes from an earlier capture. A capture whose path can match
  several nodes (`$.results[?@.name == 'demo'].id`) always captures a list,
  even when it matches one item or none.
- Each item is reported like a table row, labelled with the item (or its
  `id` or `name` field), and passes or fails on its own. The step passes
  when every item does.
- An empty list skips the step and reports it as skipped, with the reason.
- A step runs at most 100 items unless it says otherwise
  (`For each: {{old_ids}} (at most 500)`). With more than that, it fails
  before running any of them, saying how many there were.
- A step can't have both `For each:` and a table.

## Given

1. **The demo app has three resources named "demo" and one named "keep"**

## When

2. **A flow captures the ids of the "demo" resources, and a step deletes each id**

## Then

3. **Three deletes run, each reported as its own row, and only "keep" remains**

4. **When nothing matches, the delete step is skipped and says the list was empty**

5. **With more items than the limit, the step fails before deleting anything**
