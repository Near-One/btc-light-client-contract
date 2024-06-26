#!/bin/bash

# Build the wasm binary
echo "cd to contract directory"
cd ./contract || { echo "Failed to cd to contract directory"; exit 1; }

echo "Building wasm binary..."
cargo build --target wasm32-unknown-unknown --release

# Create testnet account
echo "Creating testnet account..."
account_info=$(cargo-near near create-dev-account use-random-account-id autogenerate-new-keypair save-to-legacy-keychain network-config testnet create 2>&1)

# Check if account_info is retrieved successfully
if [ -z "$account_info" ]; then
  echo "Failed to create testnet account"
  exit 1
fi

# Extract account name using awk
account_name=$(echo "$account_info" | awk '/-- create account:/ {print $4}')

# Check if account_name is extracted successfully
if [ -z "$account_name" ]; then
  echo "Failed to extract account name"
  exit 1
fi

echo "Account created: $account_name"

echo "Adding NEAR tokens to your account..."
echo "Current smart-contract use a lot of space, so we will need to faucet additonal tokens to it"
echo "Go to https://near-faucet.io/ and bump $account_name balance..."
read -p "After you are done with this press ENTER..."

echo "Let's wait a bit..."
sleep 20

# Deploy contract to testnet
echo "Deploying contract to testnet..."
near contract deploy "$account_name" use-file ./target/wasm32-unknown-unknown/release/btc_light_client_contract.wasm without-init-call network-config testnet sign-with-keychain send

# Setup relayer service
echo "cd to relayer directory..."
cd ../relayer || exit

echo "Setting up relayer service..."
key_file_path=$(echo "$account_info" | grep -oP '(?<=Key file path: ).*')
private_key=$(cat "$key_file_path" | jq -r '.private_key')

echo "Private key: $private_key"

echo "You can inspect the results of the script execution by inspecting account info at https://testnet.nearblocks.io/address/$account_name"
read -p "Press Enter to continue and remove testnet account"
near account delete-account "$account_name" beneficiary devnull.testnet network-config testnet sign-with-keychain send
