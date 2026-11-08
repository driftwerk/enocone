---
area: Getting started
tags: [init, mcp, hub, ci]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-07)
---

# One command connects a project to the AI tool, the team site and CI

Someone starting with enocone shouldn't have to read four pages of docs to
wire it up. `enocone init` sets up everything a project needs: the AI tool
learns enocone through its MCP server, runs publish to the team's site, and
CI runs the flows on every push.

```flowchart
flowchart LR
  1[A repository without enocone] --> 2[enocone init --name shop --hub https://enoc.one] --> 3[enocone.toml, .mcp.json, CI workflow]
  3 --> 4[Next steps: sign in, create shop on the site, open the AI tool]
  2 -.->|run again| 5[Nothing overwritten]
```

## Rules

- `enocone init` without `--hub` behaves as today: `enocone.toml`, an
  example flow, `.mcp.json` and `.enocone.env.example`.
- With `--hub URL`, it also writes the `[hub]` section and a
  GitHub Actions workflow that runs the flows and publishes them with
  GitHub's token (no stored secret).
- The workflow uses `driftwerk/enocone@v0`, so projects get fixes without
  editing it.
- It never overwrites a file that already exists; it says which ones it
  left alone.
- When it finishes it prints what is left to do, with the exact steps: sign
  in (`enocone login`), create the project on the site linked to the GitHub
  repository, and open the AI tool in this folder.
- The `.mcp.json` it writes works with any tool that reads that file
  (Claude Code, Cursor); the printout also shows the one-line `claude mcp
  add` for using enocone in every project.

## Open questions

- Should `init` create the project on the site itself when you're signed in
  as someone allowed to? It saves a step, but it means `init` changes
  something outside the repository.

## Given

1. **Ana has a repository acme/shop with no enocone files**
   ```exec
   # Make an empty repository for acme/shop
   run: |
     mkdir -p "{{run.tmp}}/shop" && cd "{{run.tmp}}/shop"
     git init -q . && git remote add origin https://github.com/acme/shop.git
     ls -A | grep -v '^\.git$' | wc -l | tr -d ' '
   expect: { stdout: { matches: "^0\\s*$" } }
   ```

## When

2. **She runs enocone init --name shop --hub https://enoc.one**
   ```exec
   # Run enocone init with the site
   run: |
     bin=$(realpath "{{env.ENOCONE_BIN}}") && cd "{{run.tmp}}/shop"
     "$bin" init --name shop --hub https://enoc.one 2>&1 | tee "{{run.tmp}}/first.txt"
   expect:
     stdout: { matches: "(?s)created  enocone.toml.*created  \\.mcp\\.json.*created  \\.github/workflows/enocone\\.yml" }
   ```

## Then

3. **The project has enocone.toml with a [hub] section, .mcp.json and a CI workflow that publishes**
   ```exec
   # Show the [hub] section of enocone.toml
   run: |
     cd "{{run.tmp}}/shop"
     sed -n '/^\[hub\]/,$p' enocone.toml
   expect:
     stdout: { matches: "(?s)\\[hub\\]\\s*url = \"https://enoc\\.one\"\\s*project = \"shop\"" }
   ```
   ```exec
   # Show .mcp.json
   run: |
     cat "{{run.tmp}}/shop/.mcp.json"
   expect:
     json:
       $.mcpServers.enocone.args[0]: mcp
   ```
   ```exec
   # Show the CI workflow
   run: |
     cat "{{run.tmp}}/shop/.github/workflows/enocone.yml"
   expect:
     stdout: { matches: "(?s)id-token: write.*uses: driftwerk/enocone@v0.*publish: true" }
   ```

4. **The printout lists the steps left: sign in, create shop on the site linked to acme/shop, open the AI tool**
   ```exec
   # Show the steps left in the printout
   run: |
     sed -n '/^Next:/,$p' "{{run.tmp}}/first.txt"
   expect:
     stdout: { matches: "(?s)enocone login.*create the project `shop` and link it to the GitHub repository acme/shop.*\\.mcp\\.json starts enocone's MCP server.*claude mcp add enocone --scope user -- enocone mcp" }
   ```

5. **Running init again changes nothing and says which files were already there**
   ```exec
   # Run init again and compare the files
   run: |
     bin=$(realpath "{{env.ENOCONE_BIN}}") && cd "{{run.tmp}}/shop"
     before=$(find . -path ./.git -prune -o -type f -print | sort | xargs cat | cksum)
     "$bin" init --name shop --hub https://enoc.one 2>&1 | sed -n '1,/^$/p'
     after=$(find . -path ./.git -prune -o -type f -print | sort | xargs cat | cksum)
     [ "$before" = "$after" ] && echo "files unchanged"
   expect:
     stdout: { matches: "(?s)kept     enocone\\.toml \\(already exists\\).*kept     \\.github/workflows/enocone\\.yml \\(already exists\\).*files unchanged", not: { contains: "created" } }
   ```
