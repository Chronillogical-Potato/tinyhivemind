FROM rust:1.96-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends git python3 nodejs npm \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace
