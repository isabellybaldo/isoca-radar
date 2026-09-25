# Build context: repository root. docker build -f spikes/probe/Dockerfile -t cute-probe .
FROM rust:1.98.1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p probe

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/probe /usr/local/bin/probe
WORKDIR /data
ENTRYPOINT ["probe"]
