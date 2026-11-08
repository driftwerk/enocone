---
area: Designs in the document
tags: [screen, theme, report]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-06)
---

# A mockup screen sits beside its step, drawn in the product's own style

Reviewers agree on behaviour much faster when they can see the screen. An AI
writes each mockup as a small HTML file using the project's theme, so it looks
like the product rather than a generic wireframe.

```flowchart
flowchart LR
  1[Theme and screen in the project] --> 2[Docs carry the mockup on its step] --> 3[Shown sandboxed in the product's colours]
  3 --> 4[Step stays pending until it has a check]
  1 --> 5[render-screen gives the agent a PNG]
```

## Rules

- A `screen` block names a mockup: `screen: quote` loads
  `screens/quote.html`, or `html: |` holds it inline.
- The project's `theme/` folder supplies the look: `theme.css` (CSS
  variables) and/or `tokens.json` in the W3C design-token format, which is
  turned into CSS variables. Figma variables export to that format.
- The report shows the mockup in a sandboxed frame beside the step: no
  scripts, no network, no access to the report.
- `enocone render-screen` (and the MCP tool `render_screen`) turns a screen
  into a PNG, so the agent can look at what it drew.
- A screen alone does not implement a step: the step stays Pending until it
  has an http, browser, expect, use or exec block.

## Given

1. **A project with a theme whose primary colour is #1D4BE5 and a screen “quote” that uses it**
   ```exec
   # Copy the screens project and check it is valid
   run: |
     cp -R "{{env.FIXTURES}}/screens" "{{run.tmp}}/p" && "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate
   expect: { stderr: "1 flows OK" }
   ```

## When

2. **The docs view is built for a flow whose step 2 shows the “quote” screen**
   ```exec
   # Build the docs view and read its results
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" docs --out "{{run.tmp}}/docs" && cat "{{run.tmp}}/docs/results.json"
   expect:
     json:
       $.theme: { contains: "--color-primary: #1D4BE5" }
       $.flows[0].flow.sections[1].steps[0].designs[0].screen.name: quote
       $.flows[0].flow.sections[1].steps[0].designs[0].screen.html: { contains: "Accept quote" }
   ```

## Then

3. **Step 2 shows the mockup in a sandboxed frame, coloured with the theme's primary colour**
   ```screen
   screen: report-mockup
   title: A step with its mockup
   ```
   ```browser
   - goto: "file://{{run.tmp}}/docs/index.html#flow=quote"
   - expect: { role: figure, name: "Mockup: quote" }
   - expect: { css: 'iframe[title="Mockup: Quote page"][sandbox=""][srcdoc*="--color-primary: #1D4BE5"]' }
   - screenshot
   ```

4. **The step is still pending because it has no executable block**
   ```browser
   - expect: { text: "Not implemented yet", count: 1 }
   ```

5. **render-screen writes a PNG of the “quote” screen**
   ```exec
   # Draw the quote screen as a PNG
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" render-screen quote --out "{{run.tmp}}/quote.png" && head -c 4 "{{run.tmp}}/quote.png" | tail -c 3
   expect: { stdout: PNG, stderr: quote.png }
   ```
