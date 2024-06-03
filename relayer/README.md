# Bitoin light client relayer

This is a simple implementation of a Bitcoin light client relayer. 
It is a simple server that listens for new blocks on the Bitcoin network and relays them to Near network.

## How to run

Prerequisites: You should have access to a Bitcoin full node and a Near node. Also you should have Rust installed on your machine.

1. Move config.example.toml to config.toml and fill in the required fields.
2. Run the server with `cargo run --release` in realease mode. Or you can just run with `cargo run` in debug mode.

## How to run verification flow

To check if the relayer is working correctly you can run the verification flow. This flow will check if the relayer is able to relay a block from Bitcoin to Near and check the transaction inclusion using the Merkle Proof.

We will use block 277136 as an example.

1. Run the server with `cargo run --release` in realease mode. Or you can just run with `cargo run` in debug mode and wait for some time.
2. Run the verification flow with `VERIFY_MODE="true" TRANSACTION_POSITION=0 TRANSACTION_BLOCK_HEIGHT=277136 cargo run`. This will run the verification flow for block 277136 and transaction 0. You can change block height and make sure transaction is not inlcuded in it.