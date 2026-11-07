---
area: Hosted site
tags: [hub, publish, ci]
owner: enocone
priority: high
stage: draft
---

# A run is published to enocone.xyz from a laptop or from CI

A report on someone's laptop or buried in CI artifacts is hard to discuss. `enocone
publish` sends a run to the team's site, where everyone sees the same pages.
From GitHub Actions it needs no stored secret: GitHub vouches for the workflow.

```flowchart
flowchart LR
  1[enocone login on a laptop] --> 2[enocone publish] --> 4[The run appears on the project page]
  3[CI with publish: true] --> 4
  4 --> 5[Docs and drafts update from the same upload]
  2 -.->|not a member| 6[Refused, saying who can invite]
```

## Rules

- Publishing sends `results.json` and screenshots, never HTML. The site draws
  every page itself, so a published run can't put scripts on enocone.xyz.
  Mockups stay in sandboxed frames, as in the report.
- Uploads are capped (50 MB per run) and only PNG screenshots are accepted
  beside the results.
- `enocone login` stores a personal token in the OS keychain. Tokens can be
  listed and revoked on the site.
- In GitHub Actions, the action requests an OIDC token for audience
  `https://enocone.xyz`. The site checks GitHub's signature, the issuer,
  expiry and audience, and accepts it only for projects linked to that
  repository (`acme/shop`). Forks' pull requests are refused.
- Secrets are already masked in results; the site never sees `.enocone.env`.
- The project is named in `enocone.toml` under `[hub]`:
  `url = "https://enocone.xyz"`, `project = "shop"`.

## Open questions

- Keep every run, or the last 100 per branch plus anything linked from a comment?

## Given

1. **Priya is a member of shop and has run enocone login**

## When

2. **Priya runs the prototype and publishes it**
   ```screen
   screen: hub-publish
   title: Publishing from the command line and CI
   ```

3. **The CI workflow for acme/shop runs with publish: true**

## Then

4. **Both runs appear on the shop project page, newest first**
   ```screen
   screen: hub-project
   title: Project page
   ```

5. **A token from a workflow in another repository is refused**

6. **An upload containing an HTML file is refused**
