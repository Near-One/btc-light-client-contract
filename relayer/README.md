# Bitoin light client relayer

This is a simple implementation of a Bitcoin light client relayer. 
It is a simple server that listens for new blocks on the Bitcoin network and relays them to Near network.

## General flow description
Offcahin relay works in the next way:

1. Retrieve storage state from a smart contract
2. Checks if current block belongs to any fork or a current Bitcoin main chain
3. If yes - submit the block with the correct fork ID
4. If no - submit a new fork, using new_fork_submit interface

In the meantime smart contract does two things:
1. Accepts blocks using the right interface (fork/non-fork)
2. Checks if it was a fork and maybe we need to do the reorg of chain

## How to run

Prerequisites: You should have access to a Bitcoin full node and a Near node. Also you should have Rust installed on your machine.

1. Move config.example.toml to config.toml and fill in the required fields.
2. Run the server with `cargo run --release` in realease mode. Or you can just run with `cargo run` in debug mode.

## Working with Bitcoin Prune Node
If you are running local Bitcoin Prune node and want to download block information from it you can use next set of commands.

You can get a peer list to download block info.
```shell
bitcoin-cli getpeerlist
```

Select block hash by height (i.e. HEIGHT=2) and use this blockhash in the next command
```shell
bitcoin-clie getblockhash 2
```

Download actual block content from peer.
```shell
bitcoin-cli getblockfrompeer 00000000000000027ea588641dbb07b857900a25e05797c6be40c774de2128b7 0
```

## How to run verification flow

You can just run ./scripts/run_verification_flow.sh to make sure your instance of relay is functional. 
Below is a more detailed explanation of how to use it and what commands are supported.

To check if the relayer is working correctly you can run the verification flow. This flow will check if the relayer is able to relay a block from Bitcoin to Near and check the transaction inclusion using the Merkle Proof.

We will use block 277136 as an example.

1. Run the server with `cargo run --release` in realease mode. Or you can just run with `cargo run` in debug mode and wait for some time.
2. Run the verification flow with `VERIFY_MODE="true" TRANSACTION_POSITION=0 TRANSACTION_BLOCK_HEIGHT=277136 cargo run`. This will run the verification flow for block 277136 and transaction 0. You can change block height and make sure transaction is not inlcuded in it.
3. You can also check, that wrong transaction number is not verifiable by the system. For this you can use additional env variable `FORCE_TRANSACTION_HASH=6471267463` to make sure this transaction does not exist.

## Initializing relay contract
1. Call init method
2. Set height you want to start with

## Relayer recovery algorithm
1. Wait for a new block
2. If block prev_hash does not match anything: main chain or any fork - start submitting new fork

Starting new fork subroutine:
1. Request a block on a previous height from a bitcoin node
2. Ask relay if this block exists in a main chain (get block by height method)
3. If not ask for the previous height again (repeat until you get the block that exists in main chain)
4. Start submitting blocks to the new fork 
