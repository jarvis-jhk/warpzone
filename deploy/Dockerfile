# Warpzone server built from source (no prebuilt binaries involved). Build it
# from the repository root: docker build -f deploy/Dockerfile -t warpzone-server .
# Slower to
# build than Dockerfile.release, otherwise identical.
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
RUN cargo build --release -p warpzone-server

FROM alpine:3.22
COPY --from=build /src/target/release/warpzone-server /usr/local/bin/warpzone-server
RUN adduser -D -H -u 10001 warpzone
USER 10001
ENTRYPOINT ["/usr/local/bin/warpzone-server", "--config", "/config/server.toml"]
