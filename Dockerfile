# Build: musl-based so the binary is fully static against alpine.
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

# Runtime: alpine + CA certs. Image weighs under 30 MB all in.
FROM alpine:3.20
RUN apk add --no-cache ca-certificates tzdata \
    && adduser -D -u 1000 narratarr \
    && mkdir -p /config && chown narratarr /config
COPY --from=build /src/target/release/narratarr /usr/local/bin/narratarr
VOLUME /config
WORKDIR /config
USER narratarr
# First run with an empty /config writes a commented narratarr.toml there,
# then waits for you to edit it and restart.
ENTRYPOINT ["narratarr"]
