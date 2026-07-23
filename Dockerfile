# Pinned to bookworm (Debian 12) to match the distroless runtime base below;
# the unsuffixed `slim` tag tracks whatever Debian release is current and can
# drift to a newer glibc than cc-debian12 ships, breaking the binary at
# startup (GLIBC_2.xx not found).
FROM rust:1.97-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --bin glitchtip-operator

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/glitchtip-operator /glitchtip-operator
USER nonroot
ENTRYPOINT ["/glitchtip-operator"]
