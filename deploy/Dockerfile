FROM rust:1.94.1-bookworm AS builder
WORKDIR /app

COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src

RUN cargo build --release --locked --bin personal-emr

FROM gcr.io/distroless/cc-debian12:nonroot
WORKDIR /app
COPY --from=builder /app/target/release/personal-emr /usr/local/bin/personal-emr
COPY static ./static

EXPOSE 8080
USER nonroot
ENTRYPOINT ["/usr/local/bin/personal-emr"]
