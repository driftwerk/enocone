---
area: Running flows
tags: [secrets, capture, masking]
owner: enocone
priority: high
stage: draft
---

# A captured value marked secret is masked everywhere the run records it

Some captured values are credentials: a link from an invite or password-reset
email lets anyone who holds it act as that person until it expires. Today
only `{{secret.*}}` values are masked, so such a link ends up readable in
`results.json`, the report, page snapshots, console lines and everything
published to the team site. Many teams want those values in full while
debugging, so masking a capture is something a flow or project asks for.

```flowchart
flowchart LR
  1[A flow captures an invite link marked secret and fails on its page] --> 2[It runs and is published]
  2 --> 3[The link is masked in results, report and console]
  2 --> 5[Snapshot and screenshots are still recorded]
  2 --> 6[The published run doesn't contain it]
  7[The same flow without secret: true] -.->|unchanged| a[Recorded in full, as today]
```

## Rules

- A capture can be marked secret: `capture: { invite_link: { link: "Accept", secret: true } }`
  (any capture kind: `json`, `regex`, `link`, `header`). Default off.
- From the moment it is captured, its value is masked like a `{{secret.*}}`
  value wherever the run records it:
  - captures
  - action descriptions (`goto …`)
  - request and response records
  - page snapshots and console lines
  - error messages
  - `results.json`, `index.html`, `junit.xml`, `summary.md`
  - the payload published to the team site

  Screenshots are images and are kept as they are.
- Its URL-encoded form (`%3A%2F%2F…`) is masked too, and so is the value of
  any query-string parameter that holds it, because a link's token often
  appears inside another URL.
- A project can mark captures secret by name for every flow:
  `[mask] captures = ["invite_link", "reset_link"]` in `enocone.toml`.
  Off by default.
- Masking only changes what is recorded. The flow still uses the real value.
- Without either setting, captured values are recorded in full, exactly as
  today.

## Open questions

- Should the masked text show which capture it was (`••• invite_link •••`)
  or only `•••`, as `{{secret.*}}` values do now? I recommend naming it:
  someone debugging a failed run needs to know which value went where.

## Given

1. **A flow captures an invite link from an email, marked secret, opens it, and fails on the page**

## When

2. **The flow runs, and its run is published to the team site**

## Then

3. **The link appears nowhere in results.json, the report, junit.xml, summary.md or the console output**

4. **Its URL-encoded form, inside another link on the page, is masked too**

5. **The failed step still has its page snapshot and screenshot, with the link masked in the snapshot**

6. **The run as published to the team site doesn't contain the link either**

7. **The same flow without `secret: true` records the link in full**

8. **With `[mask] captures = ["invite_link"]` in enocone.toml and no `secret: true`, the link is masked**
