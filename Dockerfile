# Multi-stage Dockerfile for Dodo Payments and Mock PSP
FROM rust:latest AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/dodo-payments
COPY . .

# Build release binaries for all workspace members
RUN cargo build --release --workspace

# Runner stage
FROM debian:bookworm-slim AS runner

RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/dodo-payments/target/release/dodo-payments /usr/local/bin/dodo-payments
COPY --from=builder /usr/src/dodo-payments/target/release/mock-psp /usr/local/bin/mock-psp

# Export default ports
EXPOSE 8080 8081

CMD ["dodo-payments"]
