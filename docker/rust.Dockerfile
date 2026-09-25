# syntax=docker/dockerfile:1
# Build context: repository root.  docker compose build scanner   (spec §8.1.1)
FROM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS chef
RUN cargo install cargo-chef --locked
WORKDIR /src

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --package scanner
COPY . .
ARG GIT_SHA=dev
ENV SCANNER_VERSION=${GIT_SHA} SOURCE_DATE_EPOCH=0
RUN cargo build --release --package scanner && mkdir -p /data && chown 65532:65532 /data

FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f
COPY --from=builder /src/target/release/scanner /usr/local/bin/scanner
COPY --from=builder --chown=65532:65532 /data /data
COPY config/scanner.toml /etc/cute-trading/scanner.toml
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/scanner"]
