# Build stage
FROM rust:1.92-slim-bookworm as builder

WORKDIR /usr/src/app
COPY . .

# Build dependencies separately for caching (optional but good practice)
# For simplicity in this mono-repo setup, we just build the workspace
RUN cargo build --release -p openhands-agent-server-rs

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
# - python3: for the agent execution
# - curl: for health checks
# - ca-certificates: for HTTPS
RUN apt-get update && apt-get install -y \
    python3 \
    python3-pip \
    curl \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from the builder stage
COPY --from=builder /usr/src/app/target/release/openhands-agent-server-rs /usr/local/bin/

# Create a workspace directory for the agent
RUN mkdir -p /workspace
WORKDIR /workspace

# Expose the API port
EXPOSE 3000

# Set environment variables
ENV RUST_LOG=info
ENV PORT=3000

# Entrypoint
CMD ["openhands-agent-server-rs"]
