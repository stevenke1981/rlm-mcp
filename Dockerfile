# rlm-mcp — multi-stage Docker build
#
# Build:
#   docker build -t rlm-mcp .
#
# Run (MCP stdio):
#   echo '{"jsonrpc":"2.0","method":"initialize","id":1}' | docker run -i rlm-mcp
#
# Run (CLI):
#   docker run -i rlm-mcp workflow --json
#
# Run with environment:
#   docker run -i \
#     -e RLM_ALLOW_NETWORK=1 \
#     -e RLM_OPENAI_API_KEY=sk-... \
#     -e RLM_CACHE_DIR=/data/cache \
#     -v rlm-data:/data \
#     rlm-mcp

# ---- Builder stage ----
FROM rust:1.85-slim-bookworm AS builder

WORKDIR /app

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

RUN cargo build --release && \
    cp target/release/rlm-mcp /rlm-mcp && \
    strip /rlm-mcp

# ---- Runtime stage ----
FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r rlm && useradd -r -g rlm -m -d /home/rlm rlm

COPY --from=builder /rlm-mcp /usr/local/bin/rlm-mcp

USER rlm
WORKDIR /home/rlm

# MCP stdio protocol
ENTRYPOINT ["rlm-mcp"]
