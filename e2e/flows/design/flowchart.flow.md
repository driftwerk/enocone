---
area: Designs in the document
tags: [flowchart, report]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-06)
---

# A flow chart in the document turns green as its steps pass

People understand a journey faster from a picture than from a list. The chart
lives in the flow document as text, so it is reviewed and diffed like the
words, and each box is tied to a step so the report can colour it with that
step's result. The chart can never drift from the steps.

## Rules

- The chart is written in a ```flowchart block using a subset of Mermaid:
  `A[Label] --> B[Label]`, dotted `-.->`, decisions `{Label}`, `%%` comments,
  and `direction LR` or `TD`.
- A box whose id is a step number (`3[Pays by card]`, or `s3`) is tied to
  that step. Boxes with other ids are just pictures.
- Before a run, tied boxes are drawn dashed (planned). After a run they are
  green (passed), red (failed), grey (skipped or not run) or violet (pending).
- Referring to a step that doesn't exist is an error that `validate` reports
  with its line number.
- The report draws the chart itself, so it still works offline from a CI artifact.
- Charts appear on the flow's page only, not in the run index (decided in review).

## Given

1. **A flow with three steps and a chart whose boxes 1, 2 and 3 are tied to them**
   ```exec
   run: |
     cp -R "{{env.FIXTURES}}/flowchart" "{{run.tmp}}/p" && "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate
   expect: { stderr: "1 flows OK" }
   ```

## When

2. **The flow runs and step 2 fails**
   ```exec
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --out "{{run.tmp}}/out"
   expect: { exit: 1 }
   ```

## Then

3. **results.json carries the chart with box 1 passed, box 2 failed and box 3 skipped**
   ```exec
   run: |
     cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       $.flows[0].flow.designs[0].chart.nodes[0].step: an-order-is-placed
       $.flows[0].flow.designs[0].chart.nodes[1].step: the-payment-is-taken
       $.flows[0].flow.designs[0].chart.nodes[3].step: { exists: false }
       $.flows[0].attempts[0].steps[0].status: passed
       $.flows[0].attempts[0].steps[1].status: failed
       $.flows[0].attempts[0].steps[2].status: skipped
   ```

4. **The report shows the chart under the flow's title with those colours**
   ```browser
   - goto: "file://{{run.tmp}}/out/index.html#flow=chart"
   - expect: { role: figure, name: Flow chart }
   - expect: { role: img, name: "Step 1: passed" }
   - expect: { role: img, name: "Step 2: failed" }
   - expect: { role: img, name: "Step 3: skipped" }
   - expect: { role: img, name: "Ask for another card" }
   - screenshot
   ```

5. **A chart box that points at step 9 of a one-step flow fails validation with its line**
   ```exec
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate "{{env.FIXTURES}}/flowchart-bad/bad-chart.flow.md"
   expect:
     exit: 2
     stderr: { matches: "bad-chart.flow.md:5: error: flowchart box `9` refers to step 9, but this flow has 1 step" }
   ```
