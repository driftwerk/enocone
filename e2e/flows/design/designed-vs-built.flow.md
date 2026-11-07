---
area: Designs in the document
tags: [screen, report]
owner: enocone
priority: normal
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-06)
---

# After a passing run, the real screen sits beside the mockup that was agreed

The point of a mockup is to agree what will be built. Once the step runs, the
report should show what was actually built next to it, so anyone can see
whether they match.

```flowchart
flowchart LR
  1[A step shows a mockup and opens the real page] --> 2[The run passes and keeps a screenshot] --> 3[Designed and Built, side by side]
```

## Rules

- A step that has both a screen and a screenshot from the run shows them side
  by side: **Designed** and **Built**.
- The screenshot is the main picture once the step passes. The mockup stays
  as design intent.

## Given

1. **A flow whose step opens the demo app's payment page and also shows the “pay” mockup**
   ```exec
   run: |
     cp -R "{{env.FIXTURES}}/designed-vs-built" "{{run.tmp}}/p" && "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate
   expect: { stderr: "1 flows OK" }
   ```

## When

2. **The flow runs against the demo app and passes**
   ```exec
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --out "{{run.tmp}}/out" && cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       $.flows[0].attempts[0].steps[0].status: passed
       $.flows[0].attempts[0].steps[0].screenshot: { exists: true }
   ```

## Then

3. **The report shows “Designed” and “Built” side by side for that step**
   ```screen
   screen: report-dvb
   title: Designed and built on one step
   ```
   ```browser
   - goto: "file://{{run.tmp}}/out/index.html#flow=pay"
   - expect: { role: group, name: Designed and built }
   - expect: { role: figure, name: "Designed: pay" }
   - expect: { role: figure, name: "Built: screenshot from this run" }
   - expect: { role: img, name: "Built: The client opens the payment page" }
   - screenshot
   ```
