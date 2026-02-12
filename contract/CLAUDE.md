# BTC Light Client Contract

## Project Overview

A Bitcoin SPV light client implemented as a NEAR Protocol smart contract. It verifies and stores block headers on-chain, enabling trustless verification of Bitcoin (and other UTXO chain) transactions without running a full node. Relayers can submit blocks and they are verified on-chain via proof-of-work validation, difficulty adjustment checks, and chain selection rules.

**Supported chains** (compile-time feature flags, mutually exclusive):
- **Bitcoin** (`bitcoin`, default) — SHA-256d PoW, 2016-block difficulty adjustment
- **Litecoin** (`litecoin`) — Scrypt PoW, modified difficulty calc
- **Dogecoin** (`dogecoin`) — Scrypt + AuxPoW (merged mining), Digishield difficulty
- **Zcash** (`zcash`) — Equihash PoW, MTP-based averaging window retarget

## Core Functions

### Write Methods

| Function | Description |
|----------|-------------|
| `init(args: InitArgs)` | Initialize contract with genesis block, network config, and GC threshold |
| `submit_blocks(headers: Vec<BlockHeader>)` | Submit block headers for on-chain PoW verification and storage. Handles main chain extension, fork tracking, and automatic reorgs |
| `run_mainchain_gc(batch_size: u64)` | Garbage collect oldest blocks beyond `gc_threshold` to manage storage costs |
| `migrate(network: Network)` | Contract state migration (e.g. V1 → V2 schema changes) |

### View Methods

| Function | Description |
|----------|-------------|
| `get_last_block_header() -> ExtendedHeader` | Current chain tip header with chain_work and height |
| `get_last_block_height() -> u64` | Current chain tip height |
| `get_block_hash_by_height(height: u64) -> Option<H256>` | Look up main chain block hash at a given height |
| `get_height_by_block_hash(blockhash: H256) -> Option<u64>` | Reverse lookup: block hash → height |
| `get_mainchain_size() -> u64` | Number of blocks currently stored on main chain |
| `get_last_n_blocks_hashes(skip, limit) -> Vec<H256>` | Paginated block hash query from tip |
| `verify_transaction_inclusion(args: ProofArgs) -> bool` | SPV proof: verify a tx is in a block via merkle proof |

## Build & Test

Requires: Rust 1.86.0, `cargo-near`, `wasm32-unknown-unknown` target.

```bash
# Build all chain variants locally (non-reproducible)
make build-local

# Run all tests (all features)
make test

# Lint (strict: -D warnings -D clippy::pedantic)
make clippy

# Format check
make fmt
```

## Architecture & Key Concepts

### On-Chain State (`BtcLightClient` in `contract/src/lib.rs`)

- **headers_pool**: `LookupMap<H256, ExtendedHeader>` — all stored headers (main chain + forks)
- **mainchain_height_to_header** / **mainchain_header_to_height**: bidirectional main chain index
- **mainchain_tip_blockhash**: current chain tip
- **gc_threshold**: max stored blocks before garbage collection kicks in

### Block Submission Flow

1. Call `submit_blocks(headers)` with one or more block headers
2. For each header, the contract:
   - Looks up the previous block in storage (rejects unattached blocks)
   - **Verifies PoW**: block hash ≤ target derived from `bits` field
   - **Checks difficulty adjustment**: validates `bits` matches the chain's retarget algorithm
   - Computes accumulated `chain_work`
3. If the block extends the main chain tip → appended directly
4. If it's a fork → stored separately; if fork's `chain_work` > main chain → **automatic reorg**

### Chain Reorganization

When a fork accumulates more work than the main chain:
1. Walk both chains back to common ancestor
2. Demote old main chain blocks (remove from height index)
3. Promote fork blocks to main chain
4. Update tip pointer

### Transaction Inclusion Verification

`verify_transaction_inclusion(ProofArgs)` — SPV proof: given a tx hash, block hash, and merkle proof, verifies the transaction is in the block by recomputing the merkle root.

**Important**: This function is vulnerable to the standard Bitcoin merkle tree second-preimage attack — it may return `true` for an internal node hash rather than a real transaction hash. Block headers do not contain the transaction count, so proof depth cannot be validated on-chain. Callers MUST validate that the `tx_id` corresponds to a valid transaction (e.g., by verifying raw transaction data) before trusting the inclusion proof.


### Garbage Collection

`run_mainchain_gc(batch_size)` removes the oldest blocks from storage when chain exceeds `gc_threshold`. Blocks older than the initial block are pruned.

## Important Build Flags

The release profile sets `overflow-checks = true` in `Cargo.toml`. This ensures all arithmetic overflows/underflows panic safely instead of wrapping. Do not remove this flag — several view functions rely on it for safe behavior with untrusted inputs.

## Chain-Specific Implementation Details

Each chain lives in its own module and is selected at compile time via feature flags:

| Chain | Module | PoW Hash | Difficulty Algorithm | Special |
|-------|--------|----------|---------------------|---------|
| Bitcoin | `bitcoin.rs` | SHA-256d | Every 2016 blocks, 14-day target | — |
| Litecoin | `litecoin.rs` | Scrypt | Every 2016 blocks, 3.5-day target | Right-shift before multiply |
| Dogecoin | `dogecoin.rs` | Scrypt | Per-block Digishield (after 145k) | AuxPoW merged mining |
| Zcash | `zcash.rs` | Equihash (200,9) | 45-block averaging window, MTP | Blossom hardfork handling |

