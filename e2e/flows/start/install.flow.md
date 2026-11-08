---
area: Getting started
tags: [install, brew, release]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-07)
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
- Builds cover macOS on Apple silicon and Linux on x86_64 and arm64. Intel
  Macs aren't built for; `cargo install` works there.
- Homebrew is the install for people and the GitHub Action for CI; there is
  no `curl | sh` script. Release archives are on the GitHub release page.

## Given

1. **A new version of enocone is tagged**
   ```exec
   # Check --version matches Cargo.toml, and that the release checks the tag
   run: |
     crate=$(sed -n 's/^version = "\(.*\)"/\1/p' ../Cargo.toml | head -1)
     [ "$("{{env.ENOCONE_BIN}}" --version)" = "enocone $crate" ] && echo "enocone --version matches Cargo.toml ($crate)"
     grep -q "doesn't match the version in Cargo.toml" ../.github/workflows/release.yml && echo "the release refuses a tag that doesn't match"
   expect:
     stdout: { matches: "(?s)--version matches Cargo.toml.*refuses a tag that doesn't match" }
   ```

## When

2. **The release builds an archive for each platform, each with the driver beside the binary**
   ```exec
   # Package an archive with the driver, as the release does
   run: |
     # What the release's Package step does, with the binary under test.
     cli=$(env -u PLAYWRIGHT_DRIVER_PATH "{{env.ENOCONE_BIN}}" doctor | sed -n 3p | tr -d ' ')
     mkdir -p "{{run.tmp}}/pkg" "{{run.tmp}}/dist"
     cp "{{env.ENOCONE_BIN}}" "{{run.tmp}}/pkg/"
     cp -R "$(dirname "$(dirname "$cli")")" "{{run.tmp}}/pkg/playwright-driver"
     tar -cf "{{run.tmp}}/dist/enocone.tar" -C "{{run.tmp}}/pkg" enocone playwright-driver
     tar -tf "{{run.tmp}}/dist/enocone.tar" | grep -E '^(enocone|playwright-driver/node|playwright-driver/package/cli\.js)$' | sort
   expect:
     stdout: "enocone\nplaywright-driver/node\nplaywright-driver/package/cli.js\n"
   ```

3. **The tap's formula is updated to the new version and checksums**
   ```exec
   # Write the Homebrew formula for the archives
   run: |
     tar="{{run.tmp}}/dist/enocone.tar"
     sha=$( (sha256sum "$tar" 2>/dev/null || shasum -a 256 "$tar") | cut -c1-64)
     for t in aarch64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do echo "$sha" > "{{run.tmp}}/dist/enocone-$t.tar.gz.sha256"; done
     sh ../.github/scripts/homebrew-formula.sh 9.8.7 "{{run.tmp}}/dist" | tee "{{run.tmp}}/enocone.rb"
     [ "$(grep -c "sha256 \"$sha\"" "{{run.tmp}}/enocone.rb")" = 3 ] && echo "each platform has the archive's checksum"
   expect:
     stdout: { matches: "(?s)version \"9\\.8\\.7\".*releases/download/v9\\.8\\.7/enocone-aarch64-apple-darwin\\.tar\\.gz.*libexec\\.install \"enocone\", \"playwright-driver\".*bin\\.install_symlink.*bundled with enocone.*each platform has the archive's checksum" }
   ```

4. **Ana installs enocone with Homebrew on her Mac**
   ```exec
   # Install the archive as the formula does
   run: |
     # The formula's install: both into libexec, and a symlink in bin.
     mkdir -p "{{run.tmp}}/brew/libexec" "{{run.tmp}}/brew/bin"
     tar -xf "{{run.tmp}}/dist/enocone.tar" -C "{{run.tmp}}/brew/libexec"
     ln -s ../libexec/enocone "{{run.tmp}}/brew/bin/enocone"
     ls "{{run.tmp}}/brew/libexec"
   expect: { stdout: "enocone\nplaywright-driver\n" }
   ```

## Then

5. **enocone doctor shows the bundled driver, and --version shows the new version**
   ```exec
   # Show the installed version and doctor
   run: |
     env -u PLAYWRIGHT_DRIVER_PATH "{{run.tmp}}/brew/bin/enocone" --version
     env -u PLAYWRIGHT_DRIVER_PATH "{{run.tmp}}/brew/bin/enocone" doctor
   expect:
     stdout: { matches: "(?s)^enocone \\S+\\n.*playwright driver: bundled with enocone\\n\\s+\\S*/brew/libexec/playwright-driver/package/cli\\.js" }
   ```

6. **After enocone install-browsers, a flow with browser steps passes**
   ```exec
   # Run a browser flow with the installed enocone
   run: |
     cp -R "{{env.FIXTURES}}/designed-vs-built" "{{run.tmp}}/p"
     env -u PLAYWRIGHT_DRIVER_PATH "{{run.tmp}}/brew/bin/enocone" --project "{{run.tmp}}/p" run --out "{{run.tmp}}/out" >/dev/null 2>&1
     cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       $.flows[0].attempts[0].steps[0].status: passed
       $.flows[0].attempts[0].steps[0].screenshot: { exists: true }
   ```
