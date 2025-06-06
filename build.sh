#!/usr/bin/env bash
set -euo pipefail

cd contract

mkdir -p ../res

variants=(bitcoin zcash dogecoin litecoin)

for variant in "${variants[@]}"; do
    echo "Building for variant: $variant"
    if [[ "$variant" == "bitcoin" ]]; then
        cargo near build reproducible-wasm
    else
        cargo near build reproducible-wasm --variant "$variant"
    fi

    wasm_path="./target/near/btc_light_client_contract.wasm"
    out_path="../res/${variant}_client.wasm"

    if [[ -f "$wasm_path" ]]; then
        mv "$wasm_path" "$out_path"
        echo "Moved $wasm_path -> $out_path"
    else
        echo "Error: $wasm_path not found for variant $variant"
        exit 1
    fi
done
