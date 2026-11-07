---
area: Designs in the document
tags: [storyboard, report]
owner: enocone
priority: normal
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-06)
---

# A storyboard shows how one screen leads to the next

A journey is a sequence of screens. A storyboard lays the mockups out in order,
with arrows labelled by the steps that move between them, so a reviewer can
follow the journey before anything is built.

```flowchart
flowchart LR
  1[Screens and transitions in the flow] --> 2[Docs carry the storyboard] --> 3[Thumbnails joined by labelled arrows]
  1 -.->|a screen has no file| 4[validate names the line]
```

## Rules

- A `storyboard` block lists transitions:
  `- { from: quote, to: pay, step: 3, on: { click: Accept quote } }`.
- Every screen it mentions must exist; a missing one is a validation error.
- The report draws the screens as thumbnails with labelled arrows.
- The same transitions drive the prototype (see “Browser steps run against the prototype”).

## Given

1. **A flow with screens “quote”, “pay” and “done” and a storyboard linking them through steps 2 and 3**
   ```exec
   run: |
     cp -R "{{env.FIXTURES}}/storyboard" "{{run.tmp}}/p" && "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate
   expect: { stderr: "1 flows OK" }
   ```

## When

2. **The docs view is built**
   ```exec
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" docs --out "{{run.tmp}}/docs" && cat "{{run.tmp}}/docs/results.json"
   expect:
     json:
       $.flows[0].flow.designs[0].kind: storyboard
       $.flows[0].flow.designs[0].screens: { len: 3 }
       $.flows[0].flow.designs[0].transitions[1].on.click: Pay
   ```

## Then

3. **The flow shows the three screens in order with arrows labelled “2” and “3”**
   ```screen
   screen: report-storyboard
   title: A flow page with its storyboard
   ```
   ```browser
   - goto: "file://{{run.tmp}}/docs/index.html#flow=journey"
   - expect: { role: figure, name: Storyboard }
   - expect: { role: figure, name: "Screen: quote" }
   - expect: { role: figure, name: "Screen: pay" }
   - expect: { role: figure, name: "Screen: done" }
   - expect: { role: img, name: "Step 2: quote to pay — click “Accept quote”" }
   - expect: { role: img, name: "Step 3: pay to done — click “Pay”" }
   - expect: { text: No screen, count: 0 }
   - screenshot
   ```

4. **A storyboard that mentions a screen with no file fails validation**
   ```exec
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate "{{env.FIXTURES}}/storyboard-bad/bad-board.flow.md"
   expect:
     exit: 2
     stderr: { matches: "bad-board.flow.md:3: error: storyboard mentions screen `nowhere`, but there is no screens/nowhere.html" }
   ```
