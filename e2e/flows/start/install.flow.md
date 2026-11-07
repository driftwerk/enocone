---
area: Getting started
tags: [install, brew, release]
owner: enocone
priority: high
stage: draft
---

# A developer installs enocone with Homebrew and runs a browser flow straight away

Most people meet enocone through an AI tool, not a Rust toolchain. Installing
should be one command on a Mac or Linux machine, and the binary it installs
must work on its own: browser steps can't depend on files that only existed
on the machine that built it.

```flowchart
flowchart LR
  1[A version is tagged] --> 2[Release builds each platform, driver included] --> 3[The tap's formula updates itself]
  3 --> 4[brew install enocone] --> 5[enocone doctor finds the bundled driver] --> 6[Browser steps pass]
```

## Rules

- `brew tap driftwerk/tap && brew install enocone` works on macOS (Apple
  Silicon) and Linux (x86_64 and ARM), including Linuxbrew on WSL2.
- Every release archive holds the `enocone` binary and the Playwright driver
  it needs (`playwright-driver/`, a pinned Node and Playwright's CLI) next to
  it. The binary finds the driver beside itself; `PLAYWRIGHT_DRIVER_PATH`
  still overrides it.
- `enocone install-browsers` then downloads Chromium, and nothing else is
  needed for browser steps. No Rust, no Node, no npm.
- `enocone doctor` says which driver it found and where.
- `enocone --version` matches the release tag; a release whose tag and
  version disagree fails before anything is published.
- Tagging a version updates the tap's formula (version, URLs, checksums)
  without anyone editing it by hand.
- The same archive is what `enocone serve` deployments and the Docker image
  use, so all three run the same files.

## Open questions

- Is macOS on Intel worth a build? GitHub still offers Intel Mac runners,
  but few new users will be on one.
- Should there also be a `curl | sh` install script for machines without
  Homebrew?

## Given

1. **A new version of enocone is tagged**

## When

2. **The release builds an archive for each platform, each with the driver beside the binary**

3. **The tap's formula is updated to the new version and checksums**

4. **Ana installs enocone with Homebrew on her Mac**

## Then

5. **enocone doctor shows the bundled driver, and --version shows the new version**

6. **After enocone install-browsers, a flow with browser steps passes**
