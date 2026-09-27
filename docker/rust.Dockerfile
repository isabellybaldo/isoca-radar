# syntax=docker/dockerfile:1
# Build context: repository root.  docker compose build migrate   (spec §8.1.1). One image, three binaries: scanner (entrypoint), api (compose.web.yml) and analyze (compose.yml), each selected by its entrypoint.
FROM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS chef
RUN cargo install cargo-chef --locked
WORKDIR /src

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --package scanner --package api --package analyze
COPY . .
# core.autocrlf on a contributor's machine can leave these files CRLF in the working tree even
# though .gitattributes pins them LF and the git index holds LF (5d deploy incident: sqlx embeds
# migration SQL at compile time and checksums the raw bytes, and config/policies.toml's sha256
# joins every paper account's run hash, so CRLF baked into the image silently changes both). Fail
# the build loudly instead of shipping a mismatched checksum — and just as loudly if a build
# context is missing these directories entirely, rather than have `grep` silently find nothing.
RUN set -e; for d in crates/store/migrations config; do test -d "$d"; done; if grep -rlI "$(printf '\r')" crates/store/migrations config; then echo "CRLF found in files baked into the image (migration checksums / policies hash)"; exit 1; fi
ARG GIT_SHA=dev
ENV SCANNER_VERSION=${GIT_SHA} SOURCE_DATE_EPOCH=0
RUN cargo build --release --package scanner --package api --package analyze && mkdir -p /data && chown 65532:65532 /data

FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f
COPY --from=builder /src/target/release/scanner /usr/local/bin/scanner
COPY --from=builder /src/target/release/api /usr/local/bin/api
COPY --from=builder /src/target/release/analyze /usr/local/bin/analyze
COPY --from=builder --chown=65532:65532 /data /data
COPY config/scanner.toml /etc/cute-trading/scanner.toml
COPY config/policies.toml /etc/cute-trading/policies.toml
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/scanner"]
