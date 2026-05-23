# Docker image for building the history-auditor workspace
FROM rust:1.94.1-slim

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    clang \
    mold \
    wget \
    && rm -rf /var/lib/apt/lists/*

RUN wget https://github.com/mozilla/sccache/releases/download/v0.8.1/sccache-v0.8.1-x86_64-unknown-linux-musl.tar.gz \
    && tar -xzf sccache-v0.8.1-x86_64-unknown-linux-musl.tar.gz \
    && mv sccache-v0.8.1-x86_64-unknown-linux-musl/sccache /usr/local/bin/ \
    && rm -rf sccache-v0.8.1-x86_64-unknown-linux-musl.tar.gz sccache-v0.8.1-x86_64-unknown-linux-musl

WORKDIR /app

COPY rust-toolchain.toml .cargo/
COPY rustfmt.toml .cargo/

RUN rustup override set 1.94.1 && \
    rustup component add rustfmt

ENV CARGO_TERM_COLOR=always
ENV RUSTFLAGS="-D warnings"

CMD ["cargo", "build", "--release"]
