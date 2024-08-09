# Bitcoin light client relayer

This is a simple implementation of a Bitcoin Light Client relayer. 
It is a simple server that listens for new blocks on the Bitcoin network and relays them to Near network.

## How to run

Prerequisites: You should have access to a Bitcoin full node and a Near node. Also you should have Rust installed on your machine.

0. Set up Rust Logger `export RUST_LOG=info`
1. Move config.example.toml to config.toml and fill in the required fields.
2. Run the server with `cargo run --release` in release mode. Or you can just run with `cargo run` in debug mode.

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
2. Run tests with `cargo test`.
