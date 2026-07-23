# Multi-stage build: compile in builder, ship minimal runtime image.
# Final stage has no shell — binary is the only executable.

FROM rust:1.86-slim-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/
RUN cargo build --release --bins

FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
WORKDIR /app
COPY --from=builder /app/target/release/sqlgate /app/sqlgate
EXPOSE 8080
USER nonroot:nonroot
ENTRYPOINT ["/app/sqlgate"]
