# enocone with Chromium, for CI systems that run containers.
#   docker build -t enocone .
#   docker run --rm -v "$PWD:/work" -w /work -e ENOCONE_VAR_API=… enocone run --env staging
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p enocone-cli

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates git && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/enocone /usr/local/bin/enocone
# The Playwright driver (Node plus its CLI) that the build assembled; the binary looks for it here.
COPY --from=build /root/.cache/playwright-rust /root/.cache/playwright-rust
# Downloads the Playwright driver and Chromium, plus the system libraries it needs.
RUN enocone install-browsers chromium --with-deps && rm -rf /var/lib/apt/lists/*
ENTRYPOINT ["enocone"]
CMD ["--help"]
