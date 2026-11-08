---
area: Flow lifecycle
tags: [cli, stage, approve]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-08)
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
- The approval records the person's name only, not their email.
- Flows are approved one by one or by naming several; there is no "approve
  every draft" option.

## Given

1. **Priya's project has a draft flow, and her git name is Priya Shah**
   ```exec
   # Copy a project with one draft flow, in a git repository where the name is Priya Shah
   run: |
     cp -R "{{env.FIXTURES}}/approve" "{{run.tmp}}/p"
     git -C "{{run.tmp}}/p" init -q && git -C "{{run.tmp}}/p" config user.name "Priya Shah"
     cp "{{run.tmp}}/p/flows/quotes/deposit.flow.md" "{{run.tmp}}/before.md"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" list
   expect: { stdout: { matches: "draft +quotes.deposit" } }
   ```

## When

2. **She lists the drafts waiting for review**
   ```exec
   # List the flows at stage draft
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" list --stage draft
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" list --stage implemented | wc -l | tr -d ' ' 
   expect: { stdout: { matches: "draft +quotes.deposit[^\\n]*\\n0" } }
   ```

3. **She approves the flow by its id**
   ```exec
   # Approve the flow by its id
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" approve quotes.deposit
   expect: { stdout: { matches: "quotes.deposit: draft → approved, approved_by: Priya Shah \\(\\d{4}-\\d\\d-\\d\\d\\)[\\s\\S]*git commit -m \"Approve quotes.deposit\" flows/quotes/deposit.flow.md" } }
   ```

## Then

4. **The flow says stage: approved and approved_by: Priya Shah with today's date, and nothing else in the file changed**
   ```exec
   # Compare the file with how it was before
   run: |
     want=$(printf '4c4,5\n< stage: draft\n---\n> stage: approved\n> approved_by: Priya Shah (%s)' "$(date +%F)")
     got=$(diff "{{run.tmp}}/before.md" "{{run.tmp}}/p/flows/quotes/deposit.flow.md")
     [ "$got" = "$want" ] && echo "only the stage changed, and approved_by was added with today's date" || printf 'unexpected change:\n%s\n' "$got"
   expect: { stdout: only the stage changed }
   ```

5. **Moving it back to review removes approved_by**
   ```exec
   # Move it back to review, and compare again
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" stage quotes.deposit review
     diff "{{run.tmp}}/before.md" "{{run.tmp}}/p/flows/quotes/deposit.flow.md" | grep -c approved_by || true
   expect: { stdout: { matches: "approved → review[\\s\\S]*\\n0\\n?$" } }
   ```

6. **Approving a flow that doesn't exist, or without a name to record, is refused with the reason**
   ```exec
   # Approve a flow that doesn't exist
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" approve quotes.nothing-here
   expect:
     exit: 1
     stderr: "no flow `quotes.nothing-here`"
   ```
   ```exec
   # Approve with no git name to record
   run: |
     git -C "{{run.tmp}}/p" config user.name ""
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" approve quotes.deposit
   expect:
     exit: 1
     stderr: "no name to record as approver"
   ```

7. **The MCP server offers no tool that approves or changes a stage**
   ```exec
   # List the MCP server's tools, and read what its guide says about approving
   run: |
     python3 "{{env.FIXTURES}}/mcp/call.py" "{{env.ENOCONE_BIN}}" "{{run.tmp}}/p" --list > "{{run.tmp}}/tools.txt"
     grep -Eqi 'approve|stage' "{{run.tmp}}/tools.txt" && echo "a tool changes stages" || echo "no tool approves or changes a stage"
     python3 "{{env.FIXTURES}}/mcp/call.py" "{{env.ENOCONE_BIN}}" "{{run.tmp}}/p" format_guide | grep -o 'ask the person to approve them themselves' 
   expect: { stdout: { matches: "no tool approves or changes a stage\\nask the person to approve them themselves" } }
   ```
