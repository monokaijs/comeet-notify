FROM rust:1.93-bookworm AS builder

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --locked --release

FROM gcr.io/distroless/cc-debian12:nonroot

COPY --from=builder /build/target/release/comeet-notify /usr/local/bin/comeet-notify

USER nonroot:nonroot
EXPOSE 3000

HEALTHCHECK --interval=10s --timeout=4s --start-period=5s --retries=3 \
  CMD ["/usr/local/bin/comeet-notify", "healthcheck"]

ENTRYPOINT ["/usr/local/bin/comeet-notify"]
