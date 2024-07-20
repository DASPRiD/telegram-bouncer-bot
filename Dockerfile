FROM rust:latest AS builder

RUN rustup target add x86_64-unknown-linux-musl \
    && apt-get update \
    && apt-get install -y \
        musl-dev \
        musl-tools \
        --no-install-recommends

WORKDIR /usr/src
RUN USER=root cargo new telegram-bouncer-bot
WORKDIR /usr/src/telegram-bouncer-bot
COPY Cargo.toml Cargo.lock ./
RUN cargo install --target x86_64-unknown-linux-musl --bin telegram-bouncer-bot --path .

COPY src ./src
RUN cargo install --target x86_64-unknown-linux-musl --bin telegram-bouncer-bot --path .

FROM scratch
COPY --from=builder /usr/local/cargo/bin/telegram-bouncer-bot .
USER 1000
CMD ["./telegram-bouncer-bot"]