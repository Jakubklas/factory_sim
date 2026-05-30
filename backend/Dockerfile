# ── Stage 1: build ───────────────────────────────────────────────────────────
# Always runs on the CI host's native arch (amd64). Cross-compiles to TARGETARCH.
FROM --platform=$BUILDPLATFORM rust:1.87-slim AS builder

ARG TARGETARCH

WORKDIR /build

# Cross-linker for arm64. OpenSSL is vendored (compiled from source) so no
# target sysroot headers are needed — pkg-config only sees the host arch.
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config gcc-aarch64-linux-gnu perl && \
    rm -rf /var/lib/apt/lists/*

# Resolve Rust target triple and add it.
RUN case "$TARGETARCH" in \
    amd64) echo "x86_64-unknown-linux-gnu"  ;; \
    arm64) echo "aarch64-unknown-linux-gnu" ;; \
    esac > /rust_target && \
    rustup target add "$(cat /rust_target)"

ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc

# Cache dependency compilation separately from source changes.
COPY Cargo.toml Cargo.lock ./
COPY plant_config/Cargo.toml plant_config/Cargo.toml
COPY backend/Cargo.toml      backend/Cargo.toml
COPY simulator/Cargo.toml    simulator/Cargo.toml
COPY codegen/Cargo.toml      codegen/Cargo.toml

RUN mkdir -p plant_config/src backend/src simulator/src codegen/src/bin && \
    echo 'pub fn _dummy() {}' > plant_config/src/lib.rs && \
    echo 'fn main() {}' > backend/src/main.rs && \
    echo 'fn main() {}' > simulator/src/main.rs && \
    echo 'fn main() {}' > codegen/src/bin/gen_compose.rs && \
    echo 'fn main() {}' > codegen/src/bin/gen_compose_device.rs && \
    TARGET=$(cat /rust_target) && \
    cargo build --release -p backend --target "$TARGET" 2>/dev/null || true

COPY plant_config/ plant_config/
COPY backend/      backend/
RUN touch plant_config/src/lib.rs backend/src/main.rs && \
    TARGET=$(cat /rust_target) && \
    cargo build --release -p backend --target "$TARGET" && \
    cp "target/$TARGET/release/backend" /usr/local/bin/backend

# ── Stage 2: runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/backend /usr/local/bin/backend

ENTRYPOINT ["/usr/local/bin/backend"]
