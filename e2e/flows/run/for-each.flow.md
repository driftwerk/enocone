---
area: Running flows
tags: [loops, capture, setup]
owner: enocone
priority: normal
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-11)
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
  several nodes (`$.results[?@.name == 'demo'].id`) captures a list when it
  matches several, and an empty list when it matches none. When it matches
  one, the capture is that one value (as it always has been, so existing
  flows keep working), and `For each:` treats it as a list of one.
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
   ```exec
   # Copy the setup project: its flows create resources in the demo app first
   run: |
     cp -R "{{env.FIXTURES}}/for-each" "{{run.tmp}}/p"
     grep -c 'name: demo' "{{run.tmp}}/p/flows/setup/tidy.flow.md"
   expect: { stdout: "3" }
   ```

## When

2. **A flow captures the ids of the "demo" resources, and a step deletes each id**
   ```exec
   # Run the setup flows and read the results
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --out "{{run.tmp}}/out" >/dev/null 2>&1
     cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       $.summary.passed: 2
       $.summary.failed: 1
   ```

## Then

3. **Three deletes run, each reported as its own row, and only "keep" remains**
   ```exec
   # Check the deletes, item by item, and what was left
   run: cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       "$.flows[?(@.flow.id=='setup.tidy')].attempts[0].steps[2].status": passed
       "$.flows[?(@.flow.id=='setup.tidy')].attempts[0].steps[2].rows": { len: 3 }
       "$.flows[?(@.flow.id=='setup.tidy')].attempts[0].steps[2].rows[0].label": { matches: "^res_" }
       "$.flows[?(@.flow.id=='setup.tidy')].attempts[0].steps[2].rows[0].result.records[0].method": DELETE
       "$.flows[?(@.flow.id=='setup.tidy')].attempts[0].steps[2].rows[2].result.records[0].status": 204
       "$.flows[?(@.flow.id=='setup.tidy')].attempts[0].steps[3].status": passed
   ```
   ```browser
   # The report lists each item with its result
   - goto: "file://{{run.tmp}}/out/index.html#flow=setup.tidy"
   - expect: { text: "For each:" }
   - expect: { role: columnheader, name: Item }
   - expect: { text: "res_" }
   - screenshot
   ```

4. **When nothing matches, the delete step is skipped and says the list was empty**
   ```exec
   # Check the delete step was skipped, and the step after it still ran
   run: cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       "$.flows[?(@.flow.id=='setup.nothing')].attempts[0].steps[2].status": skipped
       "$.flows[?(@.flow.id=='setup.nothing')].attempts[0].steps[2].note": { matches: "^skipped: `\\{\\{old_ids\\}\\}` is an empty list, so there was nothing to do$" }
       "$.flows[?(@.flow.id=='setup.nothing')].attempts[0].steps[3].status": passed
   ```

5. **With more items than the limit, the step fails before deleting anything**
   ```exec
   # Check the step failed without running any item, and all four are still there
   run: cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       "$.flows[?(@.flow.id=='setup.limit')].attempts[0].steps[2].status": failed
       "$.flows[?(@.flow.id=='setup.limit')].attempts[0].steps[2].error.message": { matches: "has 3 items, more than the 2 this step runs at most, so none were run\\. If that many is expected, write `\\(at most 3\\)` after the list$" }
       "$.flows[?(@.flow.id=='setup.limit')].attempts[0].steps[2].rows": { exists: false }
       "$.flows[?(@.flow.id=='setup.limit')].attempts[0].steps[3].status": passed
   ```
