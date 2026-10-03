# syntax=docker/dockerfile:1
# A two-stage build, used as a golden highlighting sample.

ARG RUST_VERSION=1.80
ARG APP=server

FROM rust:${RUST_VERSION}-slim AS build
WORKDIR /src
ENV CARGO_TERM_COLOR=always \
    RUSTFLAGS="-C target-cpu=native"

COPY Cargo.toml Cargo.lock ./
COPY --chown=1000:1000 crates/ ./crates/

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --bin "$APP" && \
    strip target/release/$APP

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

FROM debian:bookworm-slim AS runtime
LABEL org.opencontainers.image.title="server" \
      org.opencontainers.image.version="0.1.0"

RUN useradd --uid 1000 --create-home app
USER app:app
WORKDIR /home/app

COPY --from=build /src/target/release/server /usr/local/bin/server
ADD https://example.com/config.tar.gz /tmp/config.tar.gz

COPY <<EOT /etc/server.json
{"port": 8080, "log": "info"}
EOT

EXPOSE 8080/tcp
VOLUME ["/data"]
STOPSIGNAL SIGTERM
MAINTAINER nobody@example.com

HEALTHCHECK --interval=30s --timeout=3s \
    CMD ["server", "--check"]

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ONBUILD RUN echo "building on top"

ENTRYPOINT ["/usr/local/bin/server"]
CMD ["--port", "8080"]
