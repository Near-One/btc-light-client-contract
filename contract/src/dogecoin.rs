use crate::{BtcLightClient, BtcLightClientExt, Header, H256, U256};
use bitcoin::hashes::Hash;
use btc_types::aux::AuxData;
use btc_types::header::ExtendedHeader;
use btc_types::utils::{target_from_bits, work_from_bits};
use near_sdk::{near, require};

#[near]
impl BtcLightClient {
    pub(crate) fn get_modulated_time(&self, actual_time_taken: i64) -> u64 {
        let config = self.get_config();

        let mut modulated_time: u64 = u64::try_from(
            config.expected_time_secs as i64
                + (actual_time_taken - config.expected_time_secs as i64) / 8,
        )
            .unwrap_or(0);

        if modulated_time < (config.expected_time_secs - (config.expected_time_secs / 4)) {
            modulated_time = config.expected_time_secs - (config.expected_time_secs / 4);
        }
        if modulated_time > (config.expected_time_secs + (config.expected_time_secs * 2)) {
            modulated_time = config.expected_time_secs + (config.expected_time_secs * 2);
        }

        modulated_time
    }

    pub(crate) fn check_aux(&mut self, block_header: &Header, aux_data: &AuxData) {
        let parent_block_hash = aux_data.parent_block.block_hash();
        require!(
            self.used_aux_parent_blocks.insert(&parent_block_hash),
            "parent block already used"
        );

        let coinbase_tx = aux_data.get_coinbase_tx();
        let coinbase_tx_hash = coinbase_tx.compute_txid();

        require!(
            merkle_tools::compute_root_from_merkle_proof(
                H256::from(coinbase_tx_hash.to_raw_hash().to_byte_array()),
                0,
                &aux_data.merkle_proof,
            ) == aux_data.parent_block.merkle_root
        );

        let chain_root = merkle_tools::compute_root_from_merkle_proof(
            block_header.block_hash(),
            aux_data.chain_id,
            &aux_data.chain_merkle_proof,
        );

        require!(
            coinbase_tx
                .input
                .first()
                .unwrap()
                .script_sig
                .to_hex_string()
                .contains(&chain_root.to_string()),
            "coinbase_tx don't contain chain_root"
        );

        let pow_hash = aux_data.parent_block.block_hash_pow();
        require!(
            self.skip_pow_verification
                || U256::from_le_bytes(&pow_hash.0) <= target_from_bits(block_header.bits),
            format!("block should have correct pow")
        );
    }

    pub(crate) fn submit_block_header(
        &mut self,
        header: (Header, Option<AuxData>),
        skip_pow_verification: bool,
    ) {
        let (block_header, aux_data) = header;
        let mut skip_pow_verification = skip_pow_verification;
        if let Some(ref aux_data) = aux_data {
            self.check_aux(&block_header, aux_data);
            skip_pow_verification = true;
        }

        let prev_block_header = self.get_prev_block_header(&block_header.prev_block_hash);
        let current_block_hash = block_header.block_hash();

        let (current_block_computed_chain_work, overflow) = prev_block_header
            .chain_work
            .overflowing_add(work_from_bits(block_header.bits));
        require!(!overflow, "Addition of U256 values overflowed");

        let current_header = ExtendedHeader {
            block_header: block_header.clone().into_light(),
            block_hash: current_block_hash,
            chain_work: current_block_computed_chain_work,
            block_height: 1 + prev_block_header.block_height,
            aux_parent_block: aux_data.map(|data| data.parent_block.block_hash()),
        };

        self.submit_block_header_inner(block_header, current_header, prev_block_header, skip_pow_verification);
    }
}
