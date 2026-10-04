FROM rust:1.96-bookworm AS build

WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/examples/openhuman/target \
    cargo build --locked --manifest-path examples/openhuman/Cargo.toml --bin basic_hive \
    && cp examples/openhuman/target/debug/basic_hive /src/basic_hive \
    && strip /src/basic_hive

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libgcc-s1 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/basic_hive /usr/local/bin/basic_hive
ENV HOME=/tmp
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/basic_hive"]
