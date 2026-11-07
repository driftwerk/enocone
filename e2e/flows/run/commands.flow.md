---
area: Running flows
tags: [exec]
owner: enocone
priority: high
---

# A flow can run a command and check what it printed

Plenty of products are command-line tools, and some set-up is easiest as a
script. An `exec` step runs a command, checks its exit code and output, and
keeps values for later steps. It is off unless the project allows it, because
a flow (or an AI agent trying one out) could otherwise run anything.

## When

1. **The binary under test reports its version**
   ```exec
   run: |
     "{{env.ENOCONE_BIN}}" --version
   expect: { stdout: enocone }
   capture: { version: { regex: "enocone (\\S+)" } }
   ```

## Then

2. **A project that hasn't allowed commands refuses to run them**
   ```exec
   run: |
     mkdir -p "{{run.tmp}}/p/flows" && cp "{{env.FIXTURES}}/exec-off/enocone.toml" "{{run.tmp}}/p/" && cp "{{env.FIXTURES}}/exec-off/flow.flow.md" "{{run.tmp}}/p/flows/"
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --out "{{run.tmp}}/out" >/dev/null 2>&1
     cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       $.summary.failed: 1
       $.flows[0].attempts[0].steps[0].error.message: { contains: allow_exec }
   ```
