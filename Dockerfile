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
RUN cargo build --target x86_64-unknown-linux-musl --release

COPY src ./src
COPY i18n ./i18n
COPY ./i18n.toml ./
RUN touch src/main.rs
RUN cargo build --target x86_64-unknown-linux-musl --release
RUN mkdir /data

FROM scratch
COPY --from=builder /usr/src/telegram-bouncer-bot/target/x86_64-unknown-linux-musl/release/telegram-bouncer-bot .
COPY --from=builder --chown=1000:1000 /data ./data
USER 1000
CMD ["./telegram-bouncer-bot"]
