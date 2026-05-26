# ── Stage 1: build ───────────────────────────────────────────────────────────
FROM rust:1.87-slim AS builder

WORKDIR /build

# opcua crate links against openssl.
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

# Cache dependency compilation separately from source changes.
COPY Cargo.toml Cargo.lock ./
COPY plant_config/Cargo.toml plant_config/Cargo.toml
COPY backend/Cargo.toml      backend/Cargo.toml
COPY simulator/Cargo.toml    simulator/Cargo.toml
COPY tools/Cargo.toml        tools/Cargo.toml

# Dummy source files so cargo can resolve the workspace without the real source.
RUN mkdir -p plant_config/src backend/src simulator/src tools/src/bin && \
    echo 'pub fn _dummy() {}' > plant_config/src/lib.rs && \
    echo 'fn main() {}' > backend/src/main.rs && \
    echo 'fn main() {}' > simulator/src/main.rs && \
    echo 'fn main() {}' > tools/src/bin/gen_compose.rs && \
    cargo build --release -p simulator 2>/dev/null || true

# Copy real source and build.
COPY plant_config/ plant_config/
COPY simulator/    simulator/
RUN touch plant_config/src/lib.rs simulator/src/main.rs && \
    cargo build --release -p simulator

# ── Stage 2: runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/simulator /usr/local/bin/simulator

ENTRYPOINT ["/usr/local/bin/simulator"]
