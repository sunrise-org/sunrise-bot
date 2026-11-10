FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src target/release/sunrise-bot* target/release/deps/sunrise_bot*
COPY . .
RUN cargo build --release --locked

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/sunrise-bot /usr/local/bin/sunrise-bot
WORKDIR /app
COPY config /app/config
USER 1000:1000
ENTRYPOINT ["/usr/local/bin/sunrise-bot"]
CMD ["run"]
