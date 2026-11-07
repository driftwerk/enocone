# Running your own enocone site

`enocone serve` is the whole team site: flow documents, published runs,
members and review comments. enoc.one runs exactly this. One binary and one
data folder; it serves plain HTTP on a local port, with a proxy such as Caddy
in front for HTTPS.

## What it needs

- A small Linux machine (1 vCPU and 1 GB of RAM is plenty) with a DNS name.
- The `enocone` binary for Linux x86_64 from the
  [releases](https://github.com/driftwerk/enocone/releases), or `cargo build --release -p enocone-cli`.
- Optional: a [Postmark](https://postmarkapp.com) server token for sign-in
  emails, and a GitHub OAuth app for "Continue with GitHub".

## Configuration

Everything comes from environment variables (or the matching `--flags`; see
`enocone serve --help`). Empty values count as unset.

| Variable | Meaning |
| --- | --- |
| `ENOCONE_PUBLIC_URL` | The address people use, e.g. `https://flows.example.com`. Links in emails and secure cookies follow it. |
| `ENOCONE_LISTEN` | Where to listen, default `127.0.0.1:8080`. |
| `ENOCONE_DATA` | The data folder: `enocone.db` (SQLite) and `runs/`. Back this up. |
| `ENOCONE_OWNER_EMAIL` | The site owner. They can always sign in and create projects. |
| `ENOCONE_OWNER_GITHUB` | The owner's GitHub login, if they sign in with GitHub (their verified GitHub emails are matched against `ENOCONE_OWNER_EMAIL` too). |
| `ENOCONE_POSTMARK_TOKEN` | Postmark server token. Without it, sign-in emails are written to the log. |
| `ENOCONE_MAIL_FROM` | From address of sign-in emails (default `noreply@` the site's host). |
| `ENOCONE_GITHUB_CLIENT_ID`, `ENOCONE_GITHUB_CLIENT_SECRET` | GitHub OAuth app, for GitHub sign-in and team grants. |
| `ENOCONE_RUN_RETENTION_DAYS` | Days runs are kept, default 30. Flow documents and comments are kept. |

## Install

```sh
# as root
useradd --system --home /var/lib/enocone --shell /usr/sbin/nologin enocone
install -d -o enocone -g enocone -m 0750 /var/lib/enocone
install -d -m 0750 /etc/enocone
curl -fsSLO https://github.com/driftwerk/enocone/releases/latest/download/enocone-x86_64-unknown-linux-gnu.tar.gz
tar -xzf enocone-x86_64-unknown-linux-gnu.tar.gz && install -m 0755 enocone /usr/local/bin/enocone
```

`/etc/enocone/env` (mode 0640, group `enocone`):

```sh
ENOCONE_PUBLIC_URL=https://flows.example.com
ENOCONE_LISTEN=127.0.0.1:8080
ENOCONE_DATA=/var/lib/enocone
ENOCONE_OWNER_EMAIL=you@example.com
ENOCONE_POSTMARK_TOKEN=
ENOCONE_GITHUB_CLIENT_ID=
ENOCONE_GITHUB_CLIENT_SECRET=
```

`/etc/systemd/system/enocone.service`:

```ini
[Unit]
Description=enocone team site
After=network-online.target

[Service]
User=enocone
Group=enocone
EnvironmentFile=/etc/enocone/env
ExecStart=/usr/local/bin/enocone serve
Restart=on-failure
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ReadWritePaths=/var/lib/enocone

[Install]
WantedBy=multi-user.target
```

`/etc/caddy/Caddyfile`, so Caddy gets the certificate and forwards to the site:

```
flows.example.com {
    reverse_proxy 127.0.0.1:8080
}
```

```sh
systemctl daemon-reload && systemctl enable --now enocone && systemctl reload caddy
curl -fsS http://127.0.0.1:8080/healthz   # ok
```

## The first sign-in

Open the site and ask for a sign-in link with `ENOCONE_OWNER_EMAIL`. With
Postmark set up it arrives by email. Without it, the link is in the log:

```sh
journalctl -u enocone -n 20 --no-pager
```

Or print a link directly, as root. It reads the environment, then runs as
the `enocone` user so files in the data folder stay owned by the service:

```sh
set -a; . /etc/enocone/env; set +a
sudo --preserve-env=ENOCONE_DATA,ENOCONE_PUBLIC_URL -u enocone enocone serve --sign-in-link you@example.com
```

Quote values with spaces in `/etc/enocone/env` (`ENOCONE_MAIL_FROM="enocone <login@example.com>"`)
so the file can be sourced like this.

Then create a project and invite your team from its Members page.

## Publishing runs

In the project's `enocone.toml`:

```toml
[hub]
url = "https://flows.example.com"
project = "shop"
```

From a laptop: `enocone login`, then `enocone run` and `enocone publish`.

From GitHub Actions, link the project to its repository (`owner/name`) when
you create it, and let the workflow ask GitHub for a token:

```yaml
permissions:
  contents: read
  id-token: write
steps:
  - uses: actions/checkout@v4
  - uses: driftwerk/enocone@v1
    with:
      project: e2e
      publish: true
```

GitHub signs a short-lived token naming the repository; the site checks it
and accepts it only for the project linked to that repository. Pull requests
from forks get no token and are skipped. Elsewhere, create a token on the
Publishing page and set it as `ENOCONE_HUB_TOKEN`.

## Backups

Everything is in `ENOCONE_DATA`. For a consistent copy of the database while
the site runs:

```sh
sqlite3 /var/lib/enocone/enocone.db ".backup /var/backups/enocone.db"
```
