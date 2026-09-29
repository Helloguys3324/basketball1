# Multi-stage build for ultra-small & fast Rust binary
FROM rust:1.80-bookworm AS builder

WORKDIR /usr/src/antiscambot

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src

# Copy real source code
COPY src ./src
RUN touch src/main.rs && cargo build --release

# Final runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary from builder
COPY --from=builder /usr/src/antiscambot/target/release/antiscambot /app/antiscambot

# Copy config, dictionaries, and image templates
COPY mod_config.json* ./
COPY rust_dict.txt* ./
COPY whitelist.txt* ./
COPY dynamic_whitelist.txt* ./
COPY scam_vectors.json* ./
COPY scam_templates/ ./scam_templates/

CMD ["/app/antiscambot"]
