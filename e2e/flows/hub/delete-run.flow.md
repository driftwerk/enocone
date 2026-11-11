---
area: Hosted site
tags: [hub, runs, delete]
owner: enocone
priority: high
stage: approved
approved_by: Kingsley Hendrickse (2026-11-11)
---

# A project owner deletes a published run, and it is gone from the site at once

A published run normally leaves the site when it expires after 30 days.
When a run turns out to hold something it shouldn't, such as a credential
recorded before masking or personal data in a screenshot, an owner needs to
remove it straight away, without waiting and without server access.

```flowchart
flowchart LR
  1[A run holds something sensitive] --> 2[An owner deletes it, after confirming] --> 3[Its page, report and screenshots answer not found]
  2 --> 4[The Publishing page records who and when]
  6[CI deletes the run it published, through the API] --> 3
  5[A member who isn't an owner] -.->|refused| 2
```

## Rules

- The run page has a "Delete run" button for the project's owners. It asks
  for confirmation, naming the run, and says this can't be undone.
- `DELETE /api/projects/{slug}/runs/{id}` does the same. It is accepted from
  the project's owners, and from the CI workflow that published that run
  (the same repository and workflow, through GitHub's OIDC token).
- Deleting removes the run's row, its `results.json` and every screenshot
  stored for it. Its URLs then answer "not found".
- The flow documents and review comments stay: they belong to the project,
  not to one run. Comments made from that run's page keep their place on the
  flow.
- The project keeps a record of the deletion, showing who deleted which run
  and when, on its Publishing page.
- Members who aren't owners, reviewers and visitors to a public project can't
  delete runs, and the API refuses them.
- Only owners delete runs, including runs that members published.

## Given

1. **Priya owns the shop project and has published two runs; Sam is a member**

## When

2. **Priya deletes the first run from its page, after confirming**

## Then

3. **The run's page, report and screenshots answer not found, and the run list shows only the second run**

4. **The Publishing page records that Priya deleted the run, with the time**

5. **Sam's attempt to delete the second run through the API is refused**

6. **The CI workflow that published the second run deletes it through the API**
