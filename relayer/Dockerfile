FROM rust:1 AS build
RUN apt-get update && apt-get upgrade -y
WORKDIR /app
COPY ./relayer ./relayer
COPY ./btc-types ./btc-types
COPY ./merkle-tools ./merkle-tools
WORKDIR /app/relayer

ARG FEATURES=""
RUN if [ -n "$FEATURES" ]; then \
        cargo build --release --features "$FEATURES"; \
    else \
        cargo build --release; \
    fi


FROM rust:1-slim
COPY --from=build /app/relayer/target/release/btc-relayer /bin/relayer
ENTRYPOINT ["/bin/relayer"]