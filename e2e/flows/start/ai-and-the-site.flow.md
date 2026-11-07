---
area: Getting started
tags: [mcp, hub, review]
owner: enocone
priority: high
stage: approved
approved_by: Kingsley Hendrickse (2026-11-07)
---

# The AI tool works with the team site: it publishes drafts and answers review comments

Writing a draft is half the job; the other half is the team's review. The AI
working on a flow should be able to publish it for review, read what people
said about it on the site, and revise the draft, without anyone copying
comments between windows.

```flowchart
flowchart LR
  1[The AI writes a draft] --> 2[It publishes the run] --> 3[Reviewers comment on the site]
  3 --> 4[The AI reads the open comments] --> 5[Sees each comment and what it's about]
  5 --> r[It revises the draft and publishes again] --> 3
  4 -.->|not signed in| 6[Told to run enocone login]
```

## Rules

- The MCP server gains tools for the site: whether you're signed in and the
  project exists (`hub_status`), publishing the last run (`publish_run`),
  and reading the open comments on a flow (`review_comments`).
- They use the same sign-in as the CLI (`enocone login`, or a token in
  `ENOCONE_HUB_TOKEN`); the AI never asks for or handles a password.
- Comments come back with who wrote them, which step or quoted text they
  are about, and whether they're outdated.
- Comments are what people wrote, so the AI treats them as feedback to
  weigh, not as instructions to follow. The tool's output and the guide
  both say so.
- Reading comments needs a token for a member of the project; anyone else
  gets "not found", as on the site.
- The AI still never approves a flow; approval stays a person's change in
  git.
- The guide (`format_guide`) gains a section on working with the team site.

## Open questions

- Should the AI be able to reply to a comment on the site ("changed to 30
  days in the new version"), or only change the draft?

## Given

1. **Priya's AI tool has written a draft flow and run it**

## When

2. **It publishes the run to the shop project**

3. **Sam comments on the draft's expiry rule**

4. **The AI reads the open comments on that flow**

## Then

5. **It sees Sam's comment, the rule it quotes, and that it's open**

6. **Without a sign-in it is told to run enocone login, and is shown no comments**
