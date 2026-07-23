FROM rust:1.97-slim AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --bin glitchtip-operator

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/glitchtip-operator /glitchtip-operator
USER nonroot
ENTRYPOINT ["/glitchtip-operator"]
