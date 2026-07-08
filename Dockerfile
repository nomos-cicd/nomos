# Stage 1: Build dependencies
FROM rust:1.85 AS deps-builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
# Create dummy lib.rs and main.rs to build dependencies
RUN mkdir src && \
    echo "fn main() {println!(\"dummy\");}" > src/main.rs && \
    echo "pub fn dummy() {println!(\"dummy\");}" > src/lib.rs && \
    cargo build --release && \
    rm -rf src

# Stage 2: Build the application
FROM rust:1.85 AS app-builder
WORKDIR /app
# Copy the dependencies build artifacts
COPY --from=deps-builder /app/target target
COPY --from=deps-builder /app/Cargo.toml /app/Cargo.lock ./
# Copy the actual source code
COPY src src
# Copy templates for askama
COPY templates templates
# Build the application with static linking
RUN rustup target add x86_64-unknown-linux-musl && \
    RUSTFLAGS='-C target-feature=+crt-static' cargo build --release --target x86_64-unknown-linux-musl

# Stage 3: Create the final image
FROM docker:25.0.5-dind

# Install necessary runtime dependencies
RUN apk add --no-cache \
    openssl \
    ca-certificates \
    tzdata \
    bash \
    musl-dev \
    libc6-compat \
    gcompat \
    git-lfs

# Create a non-root user
RUN addgroup -S appgroup && adduser -S appuser -G appgroup

# Create necessary directories
RUN mkdir -p /var/lib/nomos && \
    chown -R appuser:appgroup /var/lib/nomos

# Set working directory
WORKDIR /app

# Copy the statically linked binary
COPY --from=app-builder /app/target/x86_64-unknown-linux-musl/release/nomos-rust .

# Copy data contents
COPY data/ /var/lib/nomos/

# Show binary dependencies
RUN ldd nomos-rust || true

# Change ownership of the binary and data
RUN chown -R appuser:appgroup /app /var/lib/nomos

# Switch to non-root user
USER appuser

# Make sure the binary is executable
RUN chmod +x /app/nomos-rust

# Run the application
ENTRYPOINT ["/app/nomos-rust"]
