---
area: CI
tags: [github-action, ci]
owner: enocone
priority: normal
stage: implemented
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
   ```exec
   # Copy the two projects and check both are valid
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{env.FIXTURES}}/action/pass" validate "{{env.FIXTURES}}/action/pass/flows" 2>&1
     "{{env.ENOCONE_BIN}}" --project "{{env.FIXTURES}}/action/fail" validate "{{env.FIXTURES}}/action/fail/flows" 2>&1
   expect: { stdout: { matches: "^1 flows OK, 0 errors, 0 warnings\\n1 flows OK, 0 errors, 0 warnings\\n$" } }
   ```

## When

2. **The action's script runs for each, as the action runs it**
   ```exec
   # Run the action's script in each project, with its inputs as environment variables
   run: |
     cp -R "{{env.FIXTURES}}/action" "{{run.tmp}}/a"
     bin="{{env.ENOCONE_BIN}}"; case "$bin" in /*) ;; *) bin="$PWD/$bin";; esac
     script="$PWD/action/enocone-action.sh"; [ -f "$script" ] || script="$PWD/../action/enocone-action.sh"
     export PATH="$(dirname "$bin"):$PATH"
     for k in pass fail; do
       (cd "{{run.tmp}}/a/$k" && GITHUB_OUTPUT="{{run.tmp}}/$k.out" GITHUB_STEP_SUMMARY="{{run.tmp}}/$k.md" INPUT_PROJECT="$k" INPUT_UPLOAD=on-failure INPUT_ARGS="--workers 1" bash "$script" run >/dev/null 2>&1)
       echo "$k exit $?"
     done
   expect: { stdout: "pass exit 0\nfail exit 1\n" }
   ```

## Then

3. **The passing run's outputs say outcome passed, 1 passed and 0 failed, and the job summary shows the counts**
   ```exec
   # Read the passing run's outputs and job summary
   run: cat "{{run.tmp}}/pass.out" "{{run.tmp}}/pass.md"
   expect:
     stdout: { matches: "^exit-code=0\\noutcome=passed\\npassed=1\\nfailed=0\\nreport=pass/enocone-report\\nupload=false\\n### ✅ enocone · action-pass(.|\\n)*\\| Flows \\| Passed \\| Failed (.|\\n)*\\| 1 \\| 1 \\| 0 \\|" }
   ```

4. **The failing run's outputs say outcome failed with the counts, and the report was uploaded because upload is on-failure**
   ```exec
   # Read the failing run's outputs, and check the upload step follows the upload output
   run: |
     cat "{{run.tmp}}/fail.out" "{{run.tmp}}/fail.md"
     ls "{{run.tmp}}/a/fail/enocone-report/results.json"
     grep -c "if: always() && steps.run.outputs.upload == 'true'" action.yml 2>/dev/null || grep -c "if: always() && steps.run.outputs.upload == 'true'" ../action.yml
   expect:
     stdout: { matches: "^exit-code=1\\noutcome=failed\\npassed=0\\nfailed=1\\nreport=fail/enocone-report\\nupload=true\\n(.|\\n)*\\| 1 \\| 0 \\| 1 \\|(.|\\n)*results\\.json\\n1\\n$" }
   ```

5. **No step header in the action contains the publish messages**
   ```exec
   # Count multi-line scripts and publish messages left in action.yml
   run: |
     f=action.yml; [ -f "$f" ] || f=../action.yml
     echo "scripts $(grep -c 'run: |' "$f")"
     echo "messages $(grep -c '::notice\|::warning\|Published' "$f")"
     echo "inputs in shell $(grep -E '^\s+run:' "$f" | grep -c '\${{ inputs')"
   expect:
     stdout: "scripts 2\nmessages 0\ninputs in shell 0\n"
   ```
