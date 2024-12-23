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
     /bin/bash -c "rustup target add wasm32-unknown-unknown; \
     cargo build --manifest-path contract/Cargo.toml --target wasm32-unknown-unknown --release; \
     cargo build --manifest-path contract/Cargo.toml --target wasm32-unknown-unknown --no-default-features --features bitcoin_testnet --profile bitcoin-testnet; \
     cargo build --manifest-path contract/Cargo.toml --target wasm32-unknown-unknown --no-default-features --features litecoin --profile litecoin; \
     cargo build --manifest-path contract/Cargo.toml --target wasm32-unknown-unknown --no-default-features --features litecoin_testnet  --profile litecoin-testnet; \
     cargo build --manifest-path contract/Cargo.toml --target wasm32-unknown-unknown --no-default-features --features dogecoin --profile dogecoin; \
     cargo build --manifest-path contract/Cargo.toml --target wasm32-unknown-unknown --no-default-features --features dogecoin_testnet  --profile dogecoin-testnet"

mkdir -p res
cp $DIR/contract/target/wasm32-unknown-unknown/release/btc_light_client_contract.wasm $DIR/res/btc_light_client_mainnet.wasm
cp $DIR/contract/target/wasm32-unknown-unknown/bitcoin-testnet/btc_light_client_contract.wasm $DIR/res/btc_light_client_testnet.wasm
cp $DIR/contract/target/wasm32-unknown-unknown/litecoin/btc_light_client_contract.wasm $DIR/res/btc_light_client_litecoin.wasm
cp $DIR/contract/target/wasm32-unknown-unknown/litecoin-testnet/btc_light_client_contract.wasm $DIR/res/btc_light_client_litecoin_testnet.wasm
cp $DIR/contract/target/wasm32-unknown-unknown/dogecoin/btc_light_client_contract.wasm $DIR/res/btc_light_client_dogecoin.wasm
cp $DIR/contract/target/wasm32-unknown-unknown/dogecoin-testnet/btc_light_client_contract.wasm $DIR/res/btc_light_client_dogecoin_testnet.wasm
