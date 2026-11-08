---
area: Designs in the document
tags: [flowchart, report]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-06)
---

# A flow chart in the document turns green as its steps pass

People understand a journey faster from a picture than from a list of steps.
A flow chart lives in the flow document as text, so it is reviewed and diffed
like the words around it. Each box can be tied to a step, so after a run the
report colours it with that step's result: the picture can never drift from
what the steps actually check.

```flowchart
flowchart LR
  1[Author ties boxes to steps] --> 2[The flow runs] --> 3[Results carry each box's step] --> 4[Report colours the boxes]
  1 -.->|box points at a missing step| 5[validate names the line]
```

## Rules

- The chart is written in a `flowchart` block using a subset of Mermaid:
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
   # Copy the chart project and check it is valid
   run: |
     cp -R "{{env.FIXTURES}}/flowchart" "{{run.tmp}}/p" && "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate
   expect: { stderr: "1 flows OK" }
   ```

## When

2. **The flow runs and step 2 fails**
   ```exec
   # Run the flow
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --out "{{run.tmp}}/out"
   expect: { exit: 1 }
   ```

## Then

3. **results.json carries the chart with box 1 passed, box 2 failed and box 3 skipped**
   ```exec
   # Read the chart's results
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
   ```screen
   screen: report-chart
   title: A flow page with its chart
   ```
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
   # Validate a chart that points at a missing step
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate "{{env.FIXTURES}}/flowchart-bad/bad-chart.flow.md"
   expect:
     exit: 2
     stderr: { matches: "bad-chart.flow.md:5: error: flowchart box `9` refers to step 9, but this flow has 1 step" }
   ```
