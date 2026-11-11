# enocone with Chromium, for CI systems that run containers.
#   docker build -t enocone .
#   docker run --rm -v "$PWD:/work" -w /work -e ENOCONE_VAR_API=… enocone run --env staging
FROM rust:1-trixie AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p enocone-cli

FROM debian:trixie-slim
RUN apt-get update && apt-get upgrade -y && apt-get install -y --no-install-recommends ca-certificates git && rm -rf /var/lib/apt/lists/*
# The same layout as the release archives: the Playwright driver the build
# assembled (Node plus its CLI) sits beside the binary, where it looks first.
COPY --from=build /src/target/release/enocone /opt/enocone/enocone
COPY --from=build /root/.cache/playwright-rust /tmp/pw
RUN mv "$(dirname "$(dirname "$(find /tmp/pw -path '*/package/cli.js' | head -1)")")" /opt/enocone/playwright-driver \
 && rm -rf /tmp/pw && ln -s /opt/enocone/enocone /usr/local/bin/enocone
# Downloads the Playwright driver and Chromium, plus the system libraries it needs.
RUN enocone install-browsers chromium --with-deps && rm -rf /var/lib/apt/lists/*
ENTRYPOINT ["enocone"]
CMD ["--help"]
