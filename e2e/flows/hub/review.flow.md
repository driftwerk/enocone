---
area: Hosted site
tags: [hub, review, comments]
owner: enocone
priority: high
stage: approved
approved_by: Kingsley Hendrickse (2026-11-07)
---

# Reviewers comment beside a draft flow

Drafts are written to be argued about. On enoc.one a reviewer selects a
sentence or a step and leaves a comment in the margin, the way they would on a
document. Approval stays in the flow file, in git, so the spec and the code
stay in one place; approving on the site comes later.

```flowchart
flowchart LR
  1[A draft is published] --> 2[Reviewers comment in the margin] --> 3[The author edits and publishes again]
  3 --> 4[Comments stay on their text or are marked outdated] --> 5[Settled comments are resolved]
  5 -.->|agreed| done[The author sets stage: approved in git]
```

## Rules

- Comments attach to a step or to a quoted passage of the flow's text, and
  belong to the flow's id, not to a run.
- A new version of the flow keeps a comment where its text is unchanged, and
  marks it **outdated** otherwise, still readable.
- Anyone invited can comment; members can resolve.
- Approving happens in the flow file, as today: the author sets
  `stage: approved` and `approved_by:` and commits. Authors may approve their
  own drafts.
- Later (not in this version): an Approve button on the site, and a CI job
  that opens a pull request writing the approvals into the flow files.
- Comments also appear in the HTML report when it's downloaded from the site.

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

## Then

4. **Sam's comment shows as outdated beside the new rule, still readable**

5. **Priya resolves the comment and it moves to the resolved list**
