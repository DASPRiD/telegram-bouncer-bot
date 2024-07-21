FROM clux/muslrust:stable AS chef
USER root
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --target x86_64-unknown-linux-musl --recipe-path recipe.json
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl --bin telegram-bouncer-bot && mkdir /data

FROM scratch
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/telegram-bouncer-bot .
COPY --from=builder --chown=1000:1000 /data ./data
USER 1000
CMD ["./telegram-bouncer-bot"]
