---
area: CI
tags: [github-action, ci]
owner: enocone
priority: normal
stage: approved
approved_by: Kingsley Hendrickse (2026-11-11)
---

# The GitHub Action reports the run's outcome clearly, without false alarms in its log

A job that runs the action reads its log, its summary and its outputs to
learn how the run went. Today the action's inline shell is printed in each
step's header, including its own messages such as "Publishing failed" and
"Nothing to publish", so every run's log appears to contain failures. A
job that keeps going after a failed run (`continue-on-error`) also has no
outputs telling it how many flows passed or failed.

```flowchart
flowchart LR
  1[A passing project and a failing one] --> 2[The action's script runs each]
  2 --> 3[Outputs and job summary carry the counts]
  2 --> 4[A failed run's report is uploaded]
  2 --> 5[Step headers show only the command]
```

## Rules

- The action's run and publish logic live in a script in the action, so a
  step's header shows one command, not the script and its messages.
- Its outputs are:
  - `outcome`: `passed`, `failed`, `invalid` or `error`, following the exit
    codes
  - `passed` and `failed`: counts of flows
  - `report-url`: the published run's link, when it was published
  - the existing `exit-code` and `report`
- The job summary gets the counts and, when published, the link.
- New inputs:
  - `upload`: `always` (default), `on-failure` or `never`
  - `retention-days`: passed to the artifact upload
- Inputs reach the script as environment variables, never pasted into shell
  code.
- The action's own CI runs it on a passing project and on a failing one, and
  checks the outputs of each.

## Given

1. **A project with one passing flow, and one with a failing flow**

## When

2. **The action's script runs for each, as the action runs it**

## Then

3. **The passing run's outputs say outcome passed, 1 passed and 0 failed, and the job summary shows the counts**

4. **The failing run's outputs say outcome failed with the counts, and the report was uploaded because upload is on-failure**

5. **No step header in the action contains the publish messages**
