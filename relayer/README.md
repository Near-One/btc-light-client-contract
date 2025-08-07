# Bitcoin light client relayer

This is a simple implementation of a Bitcoin Light Client relayer. 
It is a simple server that listens for new blocks on the Bitcoin network and relays them to Near network.

## How to run

Prerequisites: You should have access to a Bitcoin full node and a Near node. Also you should have Rust installed on your machine.

0. Set up Rust Logger `export RUST_LOG=info`
1. Move config.example.toml to config.toml and fill in the required fields.
2. Run the server with `cargo run --release` in release mode. Or you can just run with `cargo run` in debug mode.
3. For Zcash, you need to pass the feature flag like this: `cargo run --features "zcash"`

### Docker

To run the relayer together with a bitcoin node you can use docker compose.

0. Move config.example.toml to config.toml and fill in the required fields.
1. Create a NEAR credentials file `relayer-credentials.json` to be used by the relayer.
2. Run ```docker compose -f compose-{chain}.yaml up```.

> Because relayer depends on the bitcoin node being up-to-date, its container will fail with an error first. You'll have to wait for the bitcoin node to sync and then restart the relayer container.

Building containers locally is possible with `docker compose -f compose-{chain}.yaml build`. You can set `PLATFORM` environment variable to build relayer image for a desired platform. 

## How to run tests
### Working with Bitcoin Prune Node
To run the tests, you need to start a Bitcoin Prune Node and download block 277136.

Installation instructions for the node: https://bitcoin.org/en/full-node

In `bitcoin.conf` you should setup `rpcuser` and `rpcpassword`. 

Copy config to `~/.bitcoin` folder:
```
cp bitcoin.conf ~/.bitcoin/bitcoin.conf
```

Run bitcoin node with command
```shell
bitcoind -daemon
```

To download block 277136 run: 
```shell
 bitcoin-cli getpeerinfo
 bitcoin-cli getblockhash 277136
 bitcoin-cli getblockfrompeer <BLOCK_HASH> <PEER_ID>
```

### Run tests
0. Set up Rust Logger `export RUST_LOG=info`
1. Move config.example.toml to config.toml and fill in the required fields.
2. Run tests with `cargo test -F=integration_tests`.
