FROM rust:1.94.1-bookworm AS builder
WORKDIR /app

# 1. Dependencies only. Keyed on Cargo.toml/Cargo.lock, so this layer is a cache
#    hit until dependencies actually change — a stub main.rs gives cargo a crate
#    root to compile the dep graph against, then we drop it.
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

# 2. Real sources. A src change invalidates only from here down; target/ already
#    holds the compiled deps, so cargo recompiles just the personal-emr crate.
#    (migrations/ must be present here — sqlx::migrate! reads it at compile time.)
#    The `touch` is REQUIRED, not optional: BuildKit normalizes COPY mtimes, so
#    without it cargo thinks the real src is older than the stub build's artifacts
#    and ships the stub binary. Touching forces a rebuild of the app crate (deps,
#    whose sources live in the registry cache, are untouched and stay cached).
COPY migrations ./migrations
COPY src ./src
RUN find src -name '*.rs' -exec touch {} + \
    && cargo build --release --locked --bin personal-emr

FROM gcr.io/distroless/cc-debian12:nonroot
WORKDIR /app
COPY --from=builder /app/target/release/personal-emr /usr/local/bin/personal-emr
COPY static ./static

EXPOSE 8080
USER nonroot
ENTRYPOINT ["/usr/local/bin/personal-emr"]
