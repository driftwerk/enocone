---
area: Flow lifecycle
tags: [cli, stage, approve]
owner: enocone
priority: high
stage: draft
---

# A person approves a flow, or changes its stage, with one command

Approving a flow is a person's decision, recorded in git so the spec and the
code change together. Today that means opening the file and editing its
frontmatter by hand: easy to mistype, and easy to forget `approved_by`. A
command does it exactly, from the terminal, and shows what's waiting.

```flowchart
flowchart LR
  1[A draft is waiting] --> 2[enocone list --stage draft] --> 3[enocone approve the flow] --> 4[Frontmatter updated, nothing else]
  4 --> c[The person commits it]
  3 -.->|changed their mind| 5[enocone stage … review]
  3 -.->|unknown flow, or no name to record| 6[Refused, saying why]
  7[The AI tool] -.->|no tool for it| 3
```

## Rules

- `enocone approve <flow>…` sets `stage: approved` and
  `approved_by: <name> (<date>)` on each flow named, by its id (as `enocone
  list` shows it) or its path. The name is `git config user.name`, or
  `--by "<name>"`; with neither, it refuses rather than guess.
- `enocone stage <flow> <draft|review|approved|implemented>` moves a flow to
  any stage. Moving it back before approved removes `approved_by`; moving it
  to implemented keeps it.
- `enocone stage … implemented` refuses while any step has nothing to run,
  the same check `enocone list` shows as "3/5 steps implemented".
- Only the frontmatter lines change; the rest of the file stays exactly as
  it was. A flow without frontmatter gets one.
- Neither command commits. Each prints what changed and the `git commit`
  to record it, so approving stays an ordinary change people review.
- `enocone list --stage <stage>` lists only the flows at that stage, so
  `enocone list --stage draft` is what's waiting for review.
- The MCP server has no tool for approving or changing stage. Its guide
  says to ask the person to run `enocone approve` themselves.

## Open questions

- Should `approve` also record the approver's email, or is the name enough?
- Should there be an `enocone approve --all-drafts` for a review meeting
  where several flows are agreed at once, or is listing them enough?

## Given

1. **Priya's project has a draft flow, and her git name is Priya Shah**

## When

2. **She lists the drafts waiting for review**

3. **She approves the flow by its id**

## Then

4. **The flow says stage: approved and approved_by: Priya Shah with today's date, and nothing else in the file changed**

5. **Moving it back to review removes approved_by**

6. **Approving a flow that doesn't exist, or without a name to record, is refused with the reason**

7. **The MCP server offers no tool that approves or changes a stage**
