# syntax = docker/dockerfile:1.2

FROM bash AS get-tini

# Add Tini init-system
ENV TINI_VERSION v0.19.0
ADD https://github.com/krallin/tini/releases/download/${TINI_VERSION}/tini-static /tini
RUN chmod +x /tini

FROM clux/muslrust:stable as build

ENV CARGO_INCREMENTAL=0

WORKDIR /volume
COPY . .

RUN --mount=type=cache,target=/root/.cargo/registry --mount=type=cache,target=/volume/target \
    cargo build --locked --profile ship --target x86_64-unknown-linux-musl && \
    cp target/x86_64-unknown-linux-musl/ship/serezha-watermark-bot /volume/serezha-watermark-bot

FROM gcr.io/distroless/static

LABEL org.opencontainers.image.source https://github.com/DCNick3/serezha-watermark-bot
EXPOSE 3000

ENV ENVIRONMENT=prod

COPY --from=get-tini /tini /tini
COPY --from=build /volume/serezha-watermark-bot /serezha-watermark-bot
COPY config.yaml /

ENTRYPOINT ["/tini", "--", "/serezha-watermark-bot"]
