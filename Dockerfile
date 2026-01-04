# Build stage
FROM rust:1.92-slim-bookworm AS builder

WORKDIR /usr/src/app
COPY . .

# Build the server binary
RUN cargo build --release -p openhands-agent-server-rs

# Runtime stage
FROM debian:bookworm-slim

ARG USERNAME=openhands
ARG UID=1000
ARG GID=1000

# Install runtime dependencies and build tools
RUN apt-get update && apt-get install -y \
    python3 \
    python3-pip \
    curl \
    wget \
    git \
    sudo \
    build-essential \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create user and group
RUN groupadd -g ${GID} ${USERNAME} \
    && useradd -m -u ${UID} -g ${GID} -s /bin/bash ${USERNAME} \
    && echo "${USERNAME} ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers

# Install Rust toolchain from builder
COPY --from=builder /usr/local/cargo /usr/local/cargo
COPY --from=builder /usr/local/rustup /usr/local/rustup
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

# Install sccache
RUN arch=$(uname -m) && \
    if [ "$arch" = "x86_64" ]; then \
        url="https://github.com/mozilla/sccache/releases/download/v0.12.0/sccache-v0.12.0-x86_64-unknown-linux-musl.tar.gz"; \
    elif [ "$arch" = "aarch64" ]; then \
        url="https://github.com/mozilla/sccache/releases/download/v0.12.0/sccache-v0.12.0-aarch64-unknown-linux-musl.tar.gz"; \
    fi && \
    wget -q $url -O sccache.tar.gz \
    && tar xzf sccache.tar.gz \
    && mv sccache-v0.12.0-*/sccache /usr/local/bin/sccache \
    && chmod +x /usr/local/bin/sccache \
    && rm -rf sccache.tar.gz sccache-v0.12.0-*

# Setup sccache directory with proper permissions
RUN mkdir -p /var/cache/sccache \
    && chown -R ${USERNAME}:${USERNAME} /var/cache/sccache \
    && chmod 777 /var/cache/sccache

WORKDIR /app

# Copy the binary from the builder stage
COPY --from=builder /usr/src/app/target/release/openhands-agent-server-rs /usr/local/bin/

# Create a workspace directory for the agent
RUN mkdir -p /workspace && chown -R ${USERNAME}:${USERNAME} /workspace
WORKDIR /workspace

# Expose the API port
EXPOSE 3000

# Set environment variables
ENV RUST_LOG=info
ENV PORT=3000
ENV WORKSPACE_DIR=/workspace
ENV RUSTC_WRAPPER=/usr/local/bin/sccache
ENV SCCACHE_DIR=/var/cache/sccache
ENV CARGO_INCREMENTAL=0

USER ${USERNAME}

# Entrypoint
CMD ["openhands-agent-server-rs"]
