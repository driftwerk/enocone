# enocone: Architecture & Research

> AI-authored, business-focused end-to-end testing. Rust core, MCP for authoring, deterministic runs in CI, one beautiful self-contained HTML report.

Status: research / proposal (2026-11-06). Crate versions checked against crates.io on that date.

**Typical target.** A web app with a microservice backend described by OpenAPI specs, a single-page or Next.js frontend behind OIDC sign-in (e.g. Keycloak), and a local Docker Compose stack. The bundled demo (`crates/enocone-fixture`) is a small conveyancing app of that shape: leads, quotes sent and accepted via tokenised email links, card payment and milestones.

---

## 1. The core idea

**AI writes the tests. Nothing AI runs them.**

```
              authoring (interactive, LLM in the loop)              execution (deterministic, no LLM)
┌────────────┐   MCP    ┌──────────────────────────┐            ┌───────────────────────────────┐
│ Claude /   │ ───────▶ │ enocone mcp              │  writes    │ enocone run  (local or CI)    │
│ any agent  │ ◀─────── │  explore app, call APIs, │ ─────────▶ │  flows/*.flow.md ─▶ engine     │
└────────────┘          │  snapshot pages, dry-run │  flows/    │   ├─ http driver (reqwest)     │
                        └────────────┬─────────────┘  *.flow.md │   └─ browser driver (trait)    │
                                     │ same engine               │ ─▶ report.html  results.json  │
                                     └──────────────────────────▶│ ─▶ junit.xml    exit code     │
                                                                 └───────────────────────────────┘
```

- The agent uses the MCP server to **explore** the live app: it reads pages through accessibility snapshots, calls APIs and dry-runs steps. Then it **saves a flow document** (`*.flow.md`).
- The flow document is the source of truth. It is a human-readable business document that also runs. It is **not** generated Playwright TypeScript. If it were, the Rust runner would just wrap Node test files, and the readable report would have no source to come from.
- **The test and the report are one format.** The report is the flow document itself, rendered by the same renderer and augmented with run results: a status per step, timings, captured values and evidence. Without a run, the same renderer produces living documentation (`enocone docs`). There is nothing extra to keep in sync. What a product owner reads in a PR is what the report shows after the run.
- CI runs the specs with the same engine the MCP server uses. Runs are reproducible, cost no tokens and don't vary like an LLM does.
- "Self-healing" (later): when a locator breaks, the run records the failure. Later, an agent can use MCP to `get_failures`, re-explore the page and propose a spec patch for a human to review. It never rewrites the spec silently during a run.

## 2. One format: the flow document (what the AI writes, what humans read, what the report shows)

Tests must be shown to people (product owners, QA, support, auditors) in a form they can read. The test **is** a readable document: a Markdown file (`*.flow.md`). It reads well on GitHub, in a PR diff or in an editor. The same file is executable, and the report renders it unchanged, with results added.

The reading layer is plain prose: title, why the flow matters, and the steps as sentences under **Given / When / Then**. Under each sentence is a small fenced block saying how it's done (`http`, `browser`, `expect`). People who don't care about the detail read the sentences. In the rendered report those blocks are collapsed behind the sentence and expand to show the definition **and** what actually happened.

````markdown
---
id: quotes.client-accepts-and-pays
area: Quotes & payments
tags: [revenue, client-facing]
owner: quotes-team
priority: critical
---

# Client accepts and pays a conveyancing quote from the emailed link

Most sale instructions start when the client accepts their quote by link.
If acceptance or payment silently fails, the firm loses the instruction and
the client is left unsure whether they paid.

## Given

1. **Harbour Estates refers a sale lead for 14 Elm Road, Bristol, with client Jane Patel**
   ```http
   POST: "{{env.API}}/leads"
   auth: agency-api-key
   json:
     transaction: sale
     property: { address: "14 Elm Road, Bristol", postcode: "BS3 1AA" }
     clients: [{ name: "Jane Patel", email: "jane+{{run.id}}@example.test" }]
   capture: { lead_id: $.id }
   ```

2. **The lead has a quote: legal fee £950 + VAT and searches £228, total £1,368.00**
   ```http
   POST: "{{env.API}}/quotes"
   auth: case-handler
   json:
     lead_id: "{{lead_id}}"
     fees:
       - { name: "Legal fee", amount: 95000, vat: true }
       - { name: "Searches pack", amount: 22800 }
   expect: { status: 201, json: { $.total: 136800 } }
   capture: { quote_id: $.id, accept_token: $.accept_token }
   ```

## When

3. **The case handler opens the lead and sends the quote to the client**
   ```browser
   as: case-handler
   steps:
     - goto: "{{env.WEB}}/lead/{{lead_id}}/quotes"
     - click: { role: button, name: "Send quote" }
   ```

4. **Jane opens the acceptance link from her email and accepts the terms**
   ```browser
   - goto: "{{env.WEB}}/public/quotes/{{quote_id}}/accept/{{accept_token}}"
   - check: { label: "I accept the terms of engagement" }
   - click: { role: button, name: "Accept quote" }
   ```

5. **Jane pays £1,368.00 by card**
   ```browser
   - fill: { label: "Card number", value: "4242 4242 4242 4242" }
   - click: { role: button, name: "Pay £1,368.00" }
   ```

## Then

6. **Jane sees the payment confirmation with the amount paid**
   ```browser
   expect: { text: "£1,368.00", within: { role: region, name: "Payment confirmed" } }
   ```

7. **The quote is recorded as paid**
   ```http
   GET: "{{env.API}}/quotes/{{quote_id}}/payment-status"
   auth: admin
   eventually: 10s
   expect: { status: 200, json: { $.status: paid } }
   ```

8. **The lead shows the "Quote accepted" milestone**
   ```browser
   as: case-handler
   steps:
     - goto: "{{env.WEB}}/lead/{{lead_id}}/milestones"
     - expect: { role: checkbox, name: "Quote accepted", checked: true }
   ```

## Cleanup

- Close the test lead
  ```http
  POST: "{{env.API}}/leads/{{lead_id}}/close"
  auth: admin
  ```
````

### Rules of the format

- **Frontmatter** = metadata (id, area, tags, owner, priority, `requires`, `env`).
- **`# Title`** = the business outcome. The **paragraph under it** = why it matters (optional, but the MCP prompts ask the AI to write one).
- **`## Given / When / Then / Cleanup`** sections. Each **numbered item with a bold sentence** is one step: the bold text is the intent people read. The fenced block under it is the machine part, YAML inside, with the info string `http`, `browser`, `expect` or `use`. A step may have several blocks.
- Anything else (prose, tables, images, links to tickets) is allowed and shown as-is. It is documentation.
- Parsing uses `pulldown-cmark` to walk headings, list items and fenced blocks. The YAML in each block is validated against a JSON Schema (`schemars`). `enocone fmt` normalizes numbering and block layout, so AI-written and human-edited files look the same.
- **One variable context per flow.** `capture` writes values and `{{…}}` reads them. This is how API setup feeds browser steps, and how browser results (the quote id from the URL) feed API assertions. It is the glue for mixing HTTP and browser steps. The report shows each step's resolved values ("product_id = prd_8f2k").
- **Role/label locators first** (`role`, `name`, `label`, `text`, `test_id`), then CSS as an escape hatch. These locators are what accessibility snapshots expose to the AI, and they read like English in the document.
- **Reusable fragments**: `fragments/team/invite-case-handler.flow.md` is written in the same format, and a ```` ```use ```` block includes it. The report shows it as a nested, collapsible step.
- **Environments** (`enocone.toml`: `[env.local]`, `[env.staging]`) plus **auth profiles** that resolve tokens once per run and cache them. For example: `admin` and `case-handler` (Keycloak password/client-credentials grant), `agency-api-key` (organisation API key header) and per-flow users created in a step (`as: sam`). Browser steps with `as:` get a signed-in context injected from the same token, so they skip the login UI.
- **Unique data per run** (`{{run.id}}`, `{{faker.email}}`) so parallel CI runs don't collide.
- Not Gherkin: Gherkin matches free text to step definitions written elsewhere, which is a maintenance trap. Here the sentence and its implementation sit together in one file.

### One renderer, three outputs

| Command | Input | Output |
|---|---|---|
| `enocone docs` | `flows/**/*.flow.md` | Living documentation site/page. Same layout, no results (steps show "not run"). |
| `enocone run` | flows + live app | `report.html`: the same documents, each step marked passed/failed/skipped, with timings, resolved values and evidence |
| GitHub / editor | the raw `.flow.md` | Already readable as Markdown, so PR reviewers read the test as prose |

Because the report is the document plus results, a stakeholder who signed off on the flow in a PR sees the same sentences, in the same order, when it fails in CI.

## 3. Rust workspace layout

```
crates/
  enocone-spec      # .flow.md parser (pulldown-cmark) → typed flow model, JSON Schema for blocks, fmt, templating
  enocone-engine    # scheduler, variable context, retries, timeouts, artifacts, events stream
  enocone-http      # reqwest-based HTTP step executor, JSONPath assertions, auth profiles
  enocone-driver    # BrowserDriver trait + shared locator/assertion model
  driver-playwright # adapter over playwright-rs (default)
  driver-cdp        # adapter over chromiumoxide (no Node, Chromium only)
  driver-bidi       # adapter over rustenium (WebDriver BiDi, Firefox + Chrome)
  driver-external   # JSON-RPC-over-stdio protocol for drivers written in any language
  enocone-report    # ONE renderer: flow documents (+ optional results.json) → report.html / docs, junit.xml
  enocone-mcp       # MCP server (rmcp): stdio now, streamable HTTP for enoc.one later
  enocone-cli       # `enocone` binary: run, mcp, report, init, validate, doctor
```

### Recommended crates (crates.io, 2026-11-06)

| Need | Crate | Version | Notes |
|---|---|---|---|
| MCP server | `rmcp` (official modelcontextprotocol/rust-sdk) | 3.9.0 | `#[tool]` macros, stdio + streamable HTTP transports |
| HTTP | `reqwest` | 0.13.5 | rustls, cookies, multipart |
| Browser (default) | `playwright-rs` | 0.20.1 | Thin JSON-RPC client to the official Playwright driver; auto-waiting, Chromium/Firefox/WebKit, traces |
| Browser (no Node) | `chromiumoxide` | 0.9.1 | Raw CDP over tokio; Chromium only |
| Browser (standards) | `rustenium` | 1.1.15 | WebDriver BiDi first, optional CDP; Firefox + Chrome |
| WebDriver classic (legacy grids) | `fantoccini` | 0.22.1 | Selenium Grid / BrowserStack / Sauce compatibility |
| Markdown parsing | `pulldown-cmark` | | Walks headings, list items and fenced blocks of `.flow.md` |
| Report templating | `minijinja` | 3.0.5 | Or embed a prebuilt JS app, see §6 |
| Async / CLI / serde | `tokio`, `clap`, `serde`, `serde_yaml`/`serde_norway`, `schemars`, `jsonpath-rust`, `tracing` | | |
| Full-text search (server side, enoc.one later) | `tantivy` | 0.27.0 | Not needed for the single-file report |

## 4. Browser automation: how it works and why it is pluggable

### The trait

```rust
#[async_trait]
pub trait BrowserDriver: Send + Sync {
    fn capabilities(&self) -> Capabilities;            // browsers, tracing, video, network, a11y snapshot…
    async fn new_context(&self, opts: ContextOpts) -> Result<Box<dyn BrowserContext>>;
}

#[async_trait]
pub trait BrowserContext: Send + Sync {                // isolated cookies/storage per flow
    async fn new_page(&self) -> Result<Box<dyn Page>>;
    async fn set_storage_state(&self, s: StorageState) -> Result<()>;  // reuse API-obtained auth
    async fn close(self: Box<Self>) -> Result<Artifacts>;              // trace, video, HAR
}

#[async_trait]
pub trait Page: Send + Sync {
    async fn goto(&self, url: &str, wait: WaitUntil) -> Result<()>;
    async fn act(&self, target: &Locator, action: Action) -> Result<()>;   // click, fill, select, press, upload, hover
    async fn expect(&self, target: &Locator, cond: Condition, timeout: Duration) -> Result<()>;
    async fn snapshot(&self) -> Result<A11ySnapshot>;   // role/name tree with refs, for the AI and for failure reports
    async fn screenshot(&self, opts: ShotOpts) -> Result<Bytes>;
    async fn url(&self) -> Result<String>;
    fn events(&self) -> EventStream;                    // console, network, page errors → attached to steps
}
```

The engine works only against these traits. Each adapter translates `Locator` (role/label/text/test_id/css) into its native form:

- **Playwright adapter (default).** `playwright-rs` drives Microsoft's own Node driver over stdio JSON-RPC, so `getByRole`, auto-waiting, actionability checks, tracing and all three engines come for free, maintained upstream. The cost is a bundled Node runtime and browsers installed by version. It is the most robust choice for React/Next.js apps, where hydration timing and re-renders are the usual source of flakiness.
- **CDP adapter.** `chromiumoxide` gives a pure-Rust, single-binary path with fast startup and no Node. We implement waiting and role locators ourselves, using `Accessibility.getFullAXTree` and `DOM.resolveNode`. This is good for lightweight CI images and for the hosted runner later.
- **BiDi adapter.** `rustenium` provides W3C WebDriver BiDi, the long-term standard supported by Chrome and Firefox. Use it when users need Firefox or want to stay vendor-neutral.
- **External driver protocol.** A documented JSON-RPC 2.0 over stdio protocol that mirrors the trait (`context.new`, `page.goto`, `page.act`, `page.expect`, `page.snapshot`…). Anyone can wrap Cypress, Selenium, a device farm or an in-house tool in any language. enocone spawns `driver = { command = "node my-driver.js" }`. This answers "connect any tool" without putting every tool in our binary.

`Capabilities` lets the engine degrade gracefully, e.g. "trace not supported by this driver" instead of a crash, and lets specs declare requirements (`requires: [firefox]`).

### Making React / Next.js tests not flaky

- Prefer **role/label locators** with built-in auto-wait (actionable, visible, stable, enabled) before every action.
- **Wait for the network to go quiet** (`wait: network-idle`) plus an optional app hook: `window.__ENOCONE_READY__` or `data-hydrated` on `<body>`. A tiny `@enocone/next` helper can set it after hydration. For apps using React Query, the helper can also wait for `queryClient.isFetching() === 0`.
- **Seed data through the API, not the UI.** Then **inject auth** by calling the login API, converting the cookies/JWT into `storage_state` and opening the browser already signed in. Each test only drives the UI it is actually about.
- **Retries with evidence.** Retry a failed flow once (configurable) and mark it **flaky** rather than passed. The report shows both attempts.
- **Network interception** (Playwright/CDP), optional per step, to stub third parties like payment providers in non-prod environments.

## 5. The MCP server (authoring surface)

Built with `rmcp` over **stdio** (Claude Code, Claude Desktop, Cursor); streamable HTTP comes later for `mcp.enoc.one`. It uses the same engine as `enocone run`, so a dry run in the MCP behaves exactly as the CI run will. The exploration tools take **the same YAML as the flow blocks**, so a block the agent has tried is the block it writes.

| Group | Tool | Purpose |
|---|---|---|
| Discover | `project_info` | envs, auth profiles, flow counts by stage and area, fragments, last run |
| | `format_guide` | the flow format, the lifecycle and the working loop (`crates/enocone-mcp/src/guide.md`) |
| | `list_flows` / `get_flow` | flows with stage, steps without blocks and open questions; one flow's source and last result |
| | `describe_api` | list OpenAPI operations, or one in full with `$ref`s resolved |
| Explore | `browser_open` | a named session, optionally signed in as an auth profile (`as`), returns the a11y snapshot |
| | `browser_act` | actions written as a `browser` block; returns each outcome, captures, URL and snapshot |
| | `browser_snapshot` / `browser_screenshot` / `browser_close` | |
| | `http_request` | a request written as an `http` block, with auth profiles, expectations and captures |
| Author | `validate_flow` | diagnostics with line numbers |
| | `dry_run_flow` | run an unsaved document; per-step results, full evidence for the failing step |
| | `write_flow` | save under `flows/` or `fragments/` (refuses errors, path escapes and duplicate ids) |
| Run & triage | `run_flows` | by ids/tags/areas/text/env; writes the report and returns a summary |
| | `get_failures` | failures with error, expected vs actual, HTTP exchanges, snapshot, console and screenshot, plus the approved steps still waiting for blocks |

**Prompts** cover the lifecycle (§5a): `design_feature_flows`, `backfill_flows`, `implement_flow`, `triage_failures`.

Secrets are resolved inside the server and masked in everything it returns, so the agent sees `••••` instead of a credential.

## 5a. Lifecycle: from proposed behaviour to passing test

The same document moves through four stages (`stage:` in the frontmatter):

| stage | who | what the document holds | run? |
|---|---|---|---|
| `draft` | AI proposes | plain-English Given/When/Then, `## Rules`, `## Open questions` | no, shown as **Pending** |
| `review` | humans read it in the docs/report view and ask the AI for amendments | same | no |
| `approved` | a human signs off (`approved_by:`) | blocks are added step by step | yes; steps without blocks are **Pending** |
| `implemented` | AI, after making it pass | every step has blocks | yes (default stage) |

- **Greenfield**: `design_feature_flows` writes drafts → people review them (`enocone docs`) → the AI amends → a human approves → `implement_flow` adds blocks and builds the feature until green → `stage: implemented`.
- **Backfill** (an existing system): `backfill_flows` explores the real app and API and writes `implemented` flows of what it does today. Anything surprising goes into `## Open questions` so people can discuss it.
- `enocone run --fail-on-pending` makes pending steps fail the build. Use it once a team wants "approved means implemented" enforced in CI.

## 6. The report

- **The report is the flow documents, augmented.** Each flow renders exactly as its `.flow.md` reads: title, why, Given/When/Then sentences. A status mark, duration and resolved values are added to every step. The machine blocks are collapsed under each sentence and expand into *definition | what happened* (request/response, screenshots, console, network). A run index on top adds totals, failures first, search and filters.
- A **single self-contained `report.html`**: inline CSS, JS and run data. It opens from a CI artifact or the file system with no server. Screenshots are inlined as WebP, or kept in a sidecar `report/assets/` folder when large (configurable). Playwright traces are linked.
- Built as a small static Preact/Svelte app, compiled once and embedded in the binary with `include_str!`. At report time Rust injects the **snapshot of the flow documents that actually ran** plus `results.json` into a `<script type="application/json">` tag. This keeps the UI rich (instant search, filters, expand/collapse, keyboard nav, deep links `#flow=quotes.client-accepts-and-pays&step=7`) with no runtime server.
- **Search** runs client-side over title, area, tags, intents, error messages, owner and URL, with an in-memory index that is fine for thousands of flows. Filters cover status (passed / failed / flaky / skipped), area, tag, owner and priority.
- **Readability**: the headline is each step's business sentence from the document. Expanding a step shows the HTTP request/response (headers, body, timing), browser actions, the locator used, screenshot before/after, console errors, network calls and captured variables.
- **Failures first**: a failure summary at the top that groups by root cause (same error + step), shows expected vs actual and links to the evidence.
- **The run carries its own documents.** `results.json` embeds each flow as parsed at run time (plus a content hash of the `.flow.md`). Every step has a stable key (`step.key`, derived from its intent and kept by `enocone fmt` across renumbering). `enocone report`, PR comments and enoc.one history therefore always overlay results on the exact text that ran, never on a later edit of the file.
- Also emitted: **`results.json`** (versioned schema, `schema: enocone.results/v1`), **`junit.xml`** for CI test tabs, and a **Markdown summary** for `$GITHUB_STEP_SUMMARY` / PR comments.

## 7. CI and local use

```bash
enocone init                       # scaffold enocone.toml, flows/, .mcp.json
enocone run --env local            # run everything, open report
enocone run --tag smoke --env staging --workers 4 --retries 1
enocone run flows/quotes --headed --slow-mo 200     # watch it locally
enocone mcp                        # stdio MCP server for your agent
enocone report results.json        # re-render a report
enocone doctor                     # check browsers/driver/env connectivity
```

- Ships as **one static binary** (macOS/Linux/Windows) via `cargo-dist`, Homebrew and an install script, plus a **Docker image** with browsers preinstalled (`ghcr.io/enocone/enocone:<ver>-playwright`).
- A **GitHub Action** (`action.yml` at the repo root) installs the binary and browsers, runs the flows, uploads the report artifact and writes the step summary. A `Dockerfile` builds an image with Chromium for container-based CI.
- Exit codes: `0` passed, `1` failures, `2` config/spec errors, `3` infrastructure (browser couldn't launch, env unreachable).
- Parallelism: flows run in parallel across `--workers`, each with its own browser context. Steps inside a flow run in order. `--shard 2/4` for CI matrices.
- Secrets come from env vars / CI secrets (`{{secret.ADMIN_TOKEN}}`), are masked in reports and are never sent to the LLM through MCP.

## 8. Path to enoc.one (hosted)

Because `results.json` is versioned and the MCP server already speaks streamable HTTP, the hosted product adds features without changing the engine:

1. **Upload**: `enocone run --upload` (or the Action) pushes `results.json` + assets → history, trends, flaky-test tracking and shareable report links.
2. **Hosted MCP**: `mcp.enoc.one` with OAuth per project, so teams connect Claude without installing anything.
3. **Hosted runners**: run flows on enoc.one infra on demand, on a schedule (synthetic monitoring) or from a CI webhook. The CDP/BiDi drivers suit this best: no Node, fast cold start.
4. **Project UI**: browse and edit flows, review AI-proposed fixes and ownership/coverage by business area.

Suggested stack: Axum + Postgres + S3-compatible storage + tantivy for cross-run search, with runners in Firecracker/containers.

## 9. First-pass milestones

1. `enocone-spec` + `enocone-http` + engine: API-only flows passing, with JSON and JUnit output.
2. Playwright adapter + mixed API/browser flows + variable capture + auth injection.
3. Single-file report (from the mockups) + GitHub Action.
4. MCP server (stdio): explore, validate, dry-run, write, run, get_failures.
5. CDP adapter + external driver protocol; publish the driver protocol spec.
6. Then enoc.one: upload + history, before hosted MCP and runners.

## 10. Open questions

- Markdown + YAML blocks was chosen so the test reads as a document. Should a pure-YAML form also be accepted for machine-generated bulk flows, rendered the same way? Probably later, if at all.
- Should the Playwright adapter be the default, or should the CDP adapter be the default with Playwright opt-in? Recommendation: Playwright default for robustness, CDP for slim images.
- Visual regression (screenshot diffs): treat as an optional step type later, not core.
