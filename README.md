# enocone

Business flows written as Markdown documents that people can read, review and
discuss — and that also run as end-to-end tests (HTTP + browser). The report is
the same documents, with what happened added to each step.

An AI agent writes and maintains the flows through the enocone MCP server. That
works for features that don't exist yet: the agent drafts the flows, people
review them, and the agent builds the feature until they pass. It also works
for systems that already exist: the agent explores the app and backfills the
flows.

```markdown
# Client accepts and pays a conveyancing quote from the emailed link

## When

3. **Jane opens the acceptance link from her email and accepts the terms**
   ```browser
   - goto: /public/quotes/{{quote_id}}/accept/{{accept_token}}
   - check: { label: I accept the terms of engagement }
   - click: Accept quote
   ```
```

See [`docs/architecture.md`](docs/architecture.md) for the design and
[`crates/enocone-mcp/src/guide.md`](crates/enocone-mcp/src/guide.md) for the full format.

## Quick start

```bash
brew install driftwerk/tap/enocone  # macOS (Apple Silicon) and Linux; or: cargo install --path crates/enocone-cli
enocone install-browsers            # Chromium for browser steps
enocone doctor                      # checks the installation

enocone init                        # enocone.toml, an example flow, .mcp.json
enocone init --name shop --hub https://enoc.one   # also [hub] and a CI workflow that publishes there
enocone validate
enocone run --env local --open      # run and open the report
enocone docs --open                 # the documents only, nothing runs
enocone list                        # stage and implementation status per flow
enocone list --stage draft          # what's waiting for review
enocone approve quotes.accept       # stage: approved, approved_by: your git name and today's date
enocone stage quotes.accept review  # move a flow to any stage
enocone run --prototype             # browser steps against the mockups, before the feature exists
enocone prototype --open            # click through the mockups
enocone render-screen quote         # draw screens/quote.html with the theme, as a PNG
```

Try the demo against the bundled fixture app:

```bash
cargo run -p enocone-fixture &      # http://127.0.0.1:4010
enocone --project examples/demo run --open
```

## Lifecycle

| stage | meaning | run? |
|---|---|---|
| `draft` / `review` | proposed behaviour, being discussed | no — shown as Pending |
| `approved` | agreed; blocks are being added | steps that have blocks run; the rest are Pending |
| `implemented` (default) | every step has blocks | yes |

`--fail-on-pending` makes pending steps fail the run.

Approving is a person's change in git: `enocone approve` edits only the
frontmatter and prints the `git commit` to record it. The MCP server can't
approve; it asks you to.

## Designs in the document

Flows can carry the pictures people need to agree on behaviour. They live in
the same Markdown, and the report shows them with the results:

- **Flow charts.** A `flowchart` block in a Mermaid-style syntax. Boxes
  numbered by step turn green, red or grey with that step's result.
- **Mockup screens.** A `screen` block refers to `screens/<name>.html`. These
  are mockups written in the product's own style using the project's
  `theme/` (`theme.css`, or `tokens.json` in the W3C design-token format that
  Figma variables export to). After a run, each mockup sits next to a
  screenshot of what was built.
- **Storyboards.** A `storyboard` block lists how screens lead to each other.
  The report draws them as thumbnails with arrows labelled by step.
- **Prototype runs.** `enocone run --prototype` serves the screens at their
  paths and runs the browser steps against them. http, exec and email steps
  are skipped, and `sample:` values stand in for what they would have
  captured. The same steps later run against the real app.

````markdown
```flowchart
flowchart LR
  1[Quote sent] --> 2[Client accepts] --> 3[Pays by card]
  2 -.->|declines| chase[Reminder]
```
````

## Email and commands

- **`email` blocks** wait for a message in a sandbox mailbox, check it and
  capture its links. Set the mailbox under `[email]`: a Postmark sandbox
  server, Mailpit or MailHog. Only messages sent after the flow started
  count.
- **`exec` blocks** run a command and check its exit code, output or JSON.
  This is useful for CLIs and for seeding data. They are off unless
  `[run] allow_exec = true`. Start each with a `# title` saying what it does:
  the report shows reviewers that line, with the command folded underneath.

```toml
[email]
provider = "postmark"                       # or mailpit / mailhog
token = "{{secret.POSTMARK_SANDBOX_TOKEN}}"

[auth.staff]
type = "browser"
login = "auth/sign-in"
token = { url = "{{env.WEB}}/api/auth/session", path = "$.accessToken", refresh = "60s" }
```

`refresh` re-reads a short-lived access token from the saved browser session
when it gets that old, so long suites keep working.

## Tables of examples

A rule with many cases can be one step with a Markdown table. The step's
blocks run once per row, with that row's values as `{{row.<column>}}`, and
each row passes or fails on its own. A header ending in `?` is an expected
result:

````markdown
1. **The discount depends on the customer and the order total**

   | customer  | total | discount? |
   |-----------|------:|----------:|
   | new       |    40 |        0% |
   | returning |   120 |       10% |

   ```http
   POST: "{{env.API}}/discount"
   json: { customer: "{{row.customer}}", total: "{{row.total}}" }
   expect: { json: { $.discount: "{{row.discount}}" } }
   ```
````

The report shows the table with each row's result, and reviewers on the team
site can comment on a single row. For data with parts, a second table links
to the first: name them with `Table: orders` and `Table: items` lines, give
`items` an `orders` column, and `{{row.items}}` is that order's items.

## The team site

`enocone serve` (or enoc.one) shares flows, runs and review comments with the
team. Projects are private to their members. An owner can make an
open-source project public on its Members page: anyone with the link can
then read its flows, runs and comments (reviewers' email addresses are never
shown), and only members can comment or publish.

## With an AI agent

`enocone init` writes a `.mcp.json` that runs `enocone mcp` (stdio). The server provides:

- **Tools.** Explore with `browser_open`, `browser_act`, `http_request` and `describe_api`. Write with `validate_flow`, `dry_run_flow` and `write_flow`. Run and investigate with `run_flows` and `get_failures`.
- **Prompts.** One per stage of the lifecycle: `design_feature_flows`, `backfill_flows`, `implement_flow` and `triage_failures`.
- **The team site.** With `[hub]` in `enocone.toml`: `hub_status`, `publish_run` (so people can review a draft) and `review_comments` (what they said about it). The AI uses your `enocone login`; it never handles a password.

To use enocone in every project with Claude Code, not just where `.mcp.json` is:
`claude mcp add enocone --scope user -- enocone mcp`.

## Secrets

Write `{{secret.NAME}}` in flows or in `enocone.toml`. The value comes from the environment, or from
`.enocone.env` next to `enocone.toml` (git-ignored). It is masked in reports and in MCP
responses. Override any env variable in CI with `ENOCONE_VAR_<NAME>`.

## CI

```yaml
- uses: driftwerk/enocone@v0
  with:
    project: e2e
    env: staging
    args: --tag smoke --retries 1
  env:
    APP_USER: ${{ secrets.APP_USER }}
    APP_PASSWORD: ${{ secrets.APP_PASSWORD }}
```

The run writes `enocone-report/`: `index.html` (add `--single-file` to embed screenshots),
`results.json`, `junit.xml` and `summary.md`. It also appends to the GitHub step summary.
Exit codes: `0` passed, `1` failures, `2` flow document errors, `3` could not run.

A `Dockerfile` builds an image with Chromium for container-based CI.

## enocone tests itself

[`e2e/`](e2e) is an enocone project that specifies enocone. Each feature
started as a draft flow there, was reviewed and approved, and was then built
until the flow passed. Its fixtures are small projects that the flows run
the `enocone` binary against.

```bash
cargo build -p enocone-cli
cargo run -p enocone-fixture &      # the demo app on :4010
enocone --project e2e run --open
```

## Workspace

| crate | |
|---|---|
| `enocone-spec` | flow document parser, block model, templates, results schema |
| `enocone-engine` | runner, HTTP steps, browser steps, auth profiles, secret masking, `Driver` trait |
| `enocone-driver-playwright` | the default browser driver (playwright-rs) |
| `enocone-report` | the HTML report/docs page, JUnit, Markdown summary |
| `enocone-mcp` | MCP server and shared run orchestration |
| `enocone-cli` | the `enocone` binary |
| `enocone-fixture` | a small conveyancing app the tests run against |
