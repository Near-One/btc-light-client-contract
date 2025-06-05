FROM rust:1 AS build
RUN apt-get update && apt-get upgrade -y
WORKDIR /app
COPY ./relayer ./relayer
COPY ./btc-types ./btc-types
COPY ./merkle-tools ./merkle-tools
WORKDIR /app/relayer
RUN cargo build --release

FROM rust:1-slim
COPY --from=build /app/relayer/target/release/btc-relayer /bin/relayer
COPY --from=build /app/relayer/config.toml /config.toml
COPY --from=build /app/relayer/relayer-credentials.json /relayer-credentials.json
ENTRYPOINT ["/bin/relayer"]