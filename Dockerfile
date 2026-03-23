FROM rust:1.85-bookworm@sha256:e51d0265072d2d9d5d320f6a44dde6b9ef13653b035098febd68cce8fa7c0bc4 AS builder

WORKDIR /app

COPY Cargo.toml Cargo.toml
COPY Cargo.lock Cargo.lock
COPY src src

RUN cargo build --release --locked

FROM debian:bookworm-slim@sha256:74d56e3931e0d5a1dd51f8c8a2466d21de84a271cd3b5a733b803aa91abf4421

WORKDIR /app

COPY --from=builder /app/target/release/ztinfra-enclaveproducedhtml /app/ztinfra-enclaveproducedhtml

CMD ["/app/ztinfra-enclaveproducedhtml"]
