---
area: Running flows
tags: [prototype, storyboard]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-06)
---

# Browser steps run unchanged against the prototype before the feature exists

If the browser steps already pass against the clickable prototype, we know the
steps and the agreed screens fit together before any code is written. When
the feature is built, the very same steps run against the real app.

```flowchart
flowchart LR
  1[Draft flow with screens and a storyboard] --> 2[run --prototype serves the screens] --> 3[Browser steps pass, backend steps skipped]
  3 -.->|later, the real app without the feature| 4[The same steps fail]
```

## Rules

- `enocone run --prototype` serves the flow's screens at the paths they
  declare (`path: /public/quotes/{quote_id}/accept`) and runs only the
  browser and expect steps against them.
- Storyboard transitions become navigation: clicking “Accept quote” on
  “quote” opens “pay”.
- http and exec steps are skipped with the reason “prototype: no backend”.
  Values they would have captured come from the flow's `sample:` frontmatter.
- `enocone prototype` serves the same thing for people to click through.

## Given

1. **A draft flow with screens, a storyboard, sample values and browser steps**
   ```exec
   run: |
     cp -R "{{env.FIXTURES}}/prototype" "{{run.tmp}}/p" && "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate
   expect: { stderr: "1 flows OK" }
   ```

## When

2. **It runs with --prototype**
   ```exec
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --prototype --out "{{run.tmp}}/proto"
   expect: { exit: 0, stderr: "1 passed" }
   ```

## Then

3. **Its browser steps pass and its http steps are skipped as “prototype: no backend”**
   ```screen
   screen: proto-run
   title: A prototype run in the report
   ```
   ```exec
   run: |
     cat "{{run.tmp}}/proto/results.json"
   expect:
     json:
       $.run.prototype: true
       $.flows[0].attempts[0].steps[0].status: skipped
       $.flows[0].attempts[0].steps[0].note: "prototype: no backend"
       $.flows[0].attempts[0].steps[1].status: passed
       $.flows[0].attempts[0].steps[2].status: passed
       $.flows[0].attempts[0].steps[3].status: passed
   ```
   ```browser
   - goto: "file://{{run.tmp}}/proto/index.html#flow=accept-and-pay"
   - expect: Prototype run
   - expect: Works against the prototype
   - screenshot
   ```

4. **The same browser steps fail against an app that doesn't have the feature**
   ```exec
   run: |
     sed -i.bak 's/stage: draft/stage: approved/' "{{run.tmp}}/p/flows/accept-and-pay.flow.md" && rm "{{run.tmp}}/p/flows/accept-and-pay.flow.md.bak"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --out "{{run.tmp}}/real"
   expect: { exit: 1 }
   ```
   ```exec
   run: |
     cat "{{run.tmp}}/real/results.json"
   expect:
     json:
       $.flows[0].attempts[0].steps[0].status: passed
       $.flows[0].attempts[0].steps[1].status: failed
       $.flows[0].attempts[0].steps[1].error.message: { contains: "heading" }
   ```
