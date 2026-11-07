---
area: Getting started
tags: [init, mcp, hub, ci]
owner: enocone
priority: high
stage: draft
---

# One command connects a project to the AI tool, the team site and CI

Someone starting with enocone shouldn't have to read four pages of docs to
wire it up. `enocone init` sets up everything a project needs: the AI tool
learns enocone through its MCP server, runs publish to the team's site, and
CI runs the flows on every push.

```flowchart
flowchart LR
  1[A repository without enocone] --> 2[enocone init --hub https://enoc.one --project shop] --> 3[enocone.toml, .mcp.json, CI workflow]
  3 --> 4[Next steps: sign in, create shop on the site, open the AI tool]
  2 -.->|run again| 5[Nothing overwritten]
```

## Rules

- `enocone init` without `--hub` behaves as today: `enocone.toml`, an
  example flow, `.mcp.json` and `.enocone.env.example`.
- With `--hub URL --project SLUG`, it also writes the `[hub]` section and a
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

## When

2. **She runs enocone init --hub https://enoc.one --project shop**

## Then

3. **The project has enocone.toml with a [hub] section, .mcp.json and a CI workflow that publishes**

4. **The printout lists the steps left: sign in, create shop on the site linked to acme/shop, open the AI tool**

5. **Running init again changes nothing and says which files were already there**
