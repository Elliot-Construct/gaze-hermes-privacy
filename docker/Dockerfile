# Build the same Rust sidecar binary used by native installs
FROM rust:1.89-bookworm AS build
WORKDIR /src
COPY sidecar ./sidecar
RUN cargo build --locked --release --manifest-path sidecar/Cargo.toml

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --create-home gaze
COPY --from=build /src/sidecar/target/release/gaze-hermes-sidecar /usr/local/bin/gaze-hermes-sidecar
USER 10001:10001
EXPOSE 65113
ENTRYPOINT ["/usr/local/bin/gaze-hermes-sidecar"]