---
area: Hosted site
tags: [hub, review, comments]
owner: enocone
priority: high
stage: draft
---

# Reviewers comment beside a draft flow, and the author approves it in git

Drafts are written to be argued about. On enocone.xyz a reviewer selects a
sentence or a step and leaves a comment in the margin, the way they would on a
document. When everyone agrees, approval lands in the flow file, in git, so
the spec and the code stay in one place.

```flowchart
flowchart LR
  1[A draft is published] --> 2[Reviewers comment in the margin] --> 3[The author replies or edits]
  3 --> 2
  3 --> 4[Approve on the site] --> 5[enocone pulls approval into the flow file]
  1 -.->|the flow changed| 6[Comments stay on their text or are marked outdated]
```

## Rules

- Comments attach to a step or to a quoted passage of the flow's text, and
  belong to the flow's id, not to a run.
- A new version of the flow keeps a comment where its text is unchanged, and
  marks it **outdated** otherwise, still readable.
- Anyone invited can comment; members can resolve.
- Approving on the site records who approved and when, but doesn't edit the
  repository. `enocone review pull` writes `stage: approved` and
  `approved_by:` into the flow file for the author to commit.
- Comments also appear in the HTML report when it's downloaded from the site.

## Open questions

- Should `enocone review pull` run in CI and open a pull request with the
  approvals, or stay a manual command? Manual is simpler to start with.
- Can someone approve their own draft?

## Given

1. **Priya has published the draft “Client accepts a quote from the email”**

## When

2. **Sam comments on the rule about the 14-day expiry, and Priya replies**
   ```screen
   screen: hub-review
   title: Review page
   path: /shop/review/quotes.accept-from-email
   ```

3. **Priya changes the rule to 30 days and publishes again**

4. **Sam approves the draft**

## Then

5. **The comment shows as outdated beside the new rule, and the flow shows Approved by Sam**

6. **enocone review pull sets stage: approved and approved_by: Sam in the flow file**
