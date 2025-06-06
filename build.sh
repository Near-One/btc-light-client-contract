#!/usr/bin/env bash

# Exit script as soon as a command fails.
set -e

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"

docker run \
     --rm \
     --mount type=bind,source=$DIR,target=/host \
     --cap-add=SYS_PTRACE --security-opt seccomp=unconfined \
     -w /host \
     -e RUSTFLAGS='-C link-arg=-s' \
     rust:1.78 \
     /bin/bash -c "apt update && apt install -y clang curl build-essential pkg-config libssl-dev; \
     rustup target add wasm32-unknown-unknown; \
     cargo build --manifest-path contract/Cargo.toml --target wasm32-unknown-unknown --no-default-features --features bitcoin --profile bitcoin; \
     cargo build --manifest-path contract/Cargo.toml --target wasm32-unknown-unknown --no-default-features --features litecoin --profile litecoin; \
     cargo build --manifest-path contract/Cargo.toml --target wasm32-unknown-unknown --no-default-features --features dogecoin --profile dogecoin; \
     cargo build --manifest-path contract/Cargo.toml --target wasm32-unknown-unknown --no-default-features --features zcash --profile zcash;"

mkdir -p res
cp $DIR/contract/target/wasm32-unknown-unknown/bitcoin/btc_light_client_contract.wasm $DIR/res/btc_clinet.wasm
cp $DIR/contract/target/wasm32-unknown-unknown/litecoin/btc_light_client_contract.wasm $DIR/res/litecoin_client.wasm
cp $DIR/contract/target/wasm32-unknown-unknown/dogecoin/btc_light_client_contract.wasm $DIR/res/dogecoin_client.wasm
cp $DIR/contract/target/wasm32-unknown-unknown/zcash/btc_light_client_contract.wasm $DIR/res/zcash_client.wasm
