# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
RUN rustup target add x86_64-unknown-linux-musl

COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo chef cook --release --target x86_64-unknown-linux-musl --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release --target x86_64-unknown-linux-musl \
    && cp /app/target/x86_64-unknown-linux-musl/release/nomos-rust /usr/local/bin/nomos-rust

FROM docker:25.0.5-dind AS runtime

RUN apk add --no-cache \
    bash \
    ca-certificates \
    gcompat \
    git-lfs \
    libc6-compat \
    openssl \
    tzdata \
    && addgroup -S appgroup \
    && adduser -S appuser -G appgroup \
    && mkdir -p /app /var/lib/nomos \
    && chown -R appuser:appgroup /app /var/lib/nomos

WORKDIR /app

COPY --from=builder /usr/local/bin/nomos-rust /usr/local/bin/nomos-rust
COPY data/ /var/lib/nomos/

RUN chown -R appuser:appgroup /var/lib/nomos

USER appuser
VOLUME ["/var/lib/nomos"]

ENTRYPOINT ["/usr/local/bin/nomos-rust"]
