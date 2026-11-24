FROM rust:1-slim-bookworm AS builder
WORKDIR /app

COPY Cargo.toml ./
COPY crates/randydesk-proto ./crates/randydesk-proto
COPY crates/randydesk-relay ./crates/randydesk-relay

# Compila únicamente randydesk-proto y randydesk-relay en Linux (excluye dev-dependencies de Windows)
RUN sed -i '/"crates\/randydesk-host"/d; /"crates\/randydesk-client"/d' Cargo.toml && \
    sed -i '/^\[dev-dependencies\]/,$d' crates/randydesk-relay/Cargo.toml && \
    rm -rf crates/randydesk-relay/tests && \
    cargo build --release -p randydesk-relay

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/randydesk-relay /usr/local/bin/randydesk-relay

EXPOSE 21119
CMD ["randydesk-relay"]
