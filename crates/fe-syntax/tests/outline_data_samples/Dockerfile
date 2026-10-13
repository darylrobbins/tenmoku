# syntax=docker/dockerfile:1
ARG RUST_VERSION=1.85

FROM rust:${RUST_VERSION} AS build
WORKDIR /src
COPY . .
RUN cargo build --release

FROM --platform=linux/amd64 debian:bookworm-slim AS runtime
COPY --from=build /src/target/release/fastedit /usr/local/bin/
ENTRYPOINT ["fastedit"]

FROM scratch
COPY --from=runtime /usr/local/bin/fastedit /
