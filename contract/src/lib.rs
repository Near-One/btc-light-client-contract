use bitcoin::hashes::Hash;
use btc_types::aux::AuxData;
use btc_types::contract_args::{InitArgs, ProofArgs};
use btc_types::hash::H256;
use btc_types::header::{ExtendedHeader, Header};
use btc_types::network::{Network, NetworkConfig};
use btc_types::u256::U256;
use near_plugins::{
    access_control, pause, AccessControlRole, AccessControllable, Pausable, Upgradable,
};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, LookupSet};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, log, near, require, NearToken, PanicOnDefault, Promise, PromiseOrValue};

pub(crate) const ERR_KEY_NOT_EXIST: &str = "ERR_KEY_NOT_EXIST";

/// Define roles for access control of `Pausable` features. Accounts which are
/// granted a role are authorized to execute the corresponding action.
#[derive(AccessControlRole, Deserialize, Serialize, Copy, Clone)]
#[serde(crate = "near_sdk::serde")]
pub enum Role {
    /// May pause and unpause features.
    PauseManager,
    /// Allows to use contract API even after contract is paused
    UnrestrictedSubmitBlocks,
    // Allows to use `run_mainchain_gc` API on a paused contract
    UnrestrictedRunGC,
    /// May successfully call any of the protected `Upgradable` methods since below it is passed to
    /// every attribute of `access_control_roles`.
    ///
    /// Using this pattern grantees of a single role are authorized to call all `Upgradable`methods.
    DAO,
    /// May successfully call `Upgradable::up_stage_code`, but none of the other protected methods,
    /// since below is passed only to the `code_stagers` attribute.
    ///
    /// Using this pattern grantees of a role are authorized to call only one particular protected
    /// `Upgradable` method.
    CodeStager,
    /// May successfully call `Upgradable::up_deploy_code`, but none of the other protected methods,
    /// since below is passed only to the `code_deployers` attribute.
    ///
    /// Using this pattern grantees of a role are authorized to call only one particular protected
    /// `Upgradable` method.
    CodeDeployer,
    /// May successfully call `Upgradable` methods to initialize and update the staging duration
    /// since below it is passed to the attributes `duration_initializers`,
    /// `duration_update_stagers`, and `duration_update_appliers`.
    ///
    /// Using this pattern grantees of a single role are authorized to call multiple (but not all)
    /// protected `Upgradable` methods.
    DurationManager,
}
#[derive(BorshSerialize, near_sdk::BorshStorageKey)]
enum StorageKey {
    MainchainHeightToHeader,
    MainchainHeaderToHeight,
    HeadersPool,
    AuxParentBlocks,
}

/// Contract implementing Bitcoin light client.
/// See README.md for more details about features and implementation logic behind the code.
/// This contract could work in a pairing with an external off-chain relay service. To learn more about
/// relay, take a look at the relay service documentation.
#[access_control(role_type(Role))]
#[near(contract_state)]
#[derive(Pausable, Upgradable, PanicOnDefault)]
#[pausable(manager_roles(Role::PauseManager))]
#[upgradable(access_control_roles(
    code_stagers(Role::CodeStager, Role::DAO),
    code_deployers(Role::CodeDeployer, Role::DAO),
    duration_initializers(Role::DurationManager, Role::DAO),
    duration_update_stagers(Role::DurationManager, Role::DAO),
    duration_update_appliers(Role::DurationManager, Role::DAO),
))]
pub struct BtcLightClient {
    // A pair of lookup maps that allows to find header by height and height by header
    mainchain_height_to_header: LookupMap<u64, H256>,
    mainchain_header_to_height: LookupMap<H256, u64>,

    // Block with the highest chainWork, i.e., blockchain tip, you can find latest height inside of it
    mainchain_tip_blockhash: H256,

    // The oldest block in main chain we store
    mainchain_initial_blockhash: H256,

    // Mapping of block hashes to block headers (ALL ever submitted, i.e., incl. forks)
    headers_pool: LookupMap<H256, ExtendedHeader>,

    // If we should run all the block checks or not
    skip_pow_verification: bool,

    // GC threshold - how many blocks we would like to store in memory, and GC the older ones
    gc_threshold: u64,

    // Used only for networks with AuxPoW (Dogecoin). These are the hashes of already used parent blocks (Litecoin blocks for Dogecoin)
    used_aux_parent_blocks: LookupSet<H256>,
}

#[near]
impl BtcLightClient {
    /// Recommended initialization parameters:
    /// * `genesis_block_height % blocks_per_adjustment == 0`: The genesis block height must be divisible by `blocks_per_adjustment` to align with difficulty adjustment cycles.
    /// * The `genesis_block` must be at least 144 blocks earlier than the last block. 144 is the approximate number of blocks generated in one day.
    /// * `skip_pow_verification = false`: Should be set to `false` for standard use. Set to `true` only for testing purposes.
    /// * `gc_threshold = 52704`: This is the approximate number of blocks generated in a year.
    #[init]
    #[private]
    #[must_use]
    pub fn init(args: InitArgs) -> Self {
        let mut contract = Self {
            mainchain_height_to_header: LookupMap::new(StorageKey::MainchainHeightToHeader),
            mainchain_header_to_height: LookupMap::new(StorageKey::MainchainHeaderToHeight),
            headers_pool: LookupMap::new(StorageKey::HeadersPool),
            mainchain_initial_blockhash: H256::default(),
            mainchain_tip_blockhash: H256::default(),
            skip_pow_verification: args.skip_pow_verification,
            gc_threshold: args.gc_threshold,
            used_aux_parent_blocks: LookupSet::new(StorageKey::AuxParentBlocks),
        };

        // Make the contract itself super admin. This allows us to grant any role in the
        // constructor.
        near_sdk::require!(
            contract.acl_init_super_admin(env::current_account_id()),
            "Failed to initialize super admin",
        );

        contract.init_genesis(
            args.genesis_block,
            &args.genesis_block_hash,
            args.genesis_block_height,
        );

        contract
    }

    #[payable]
    #[pause(except(roles(Role::UnrestrictedSubmitBlocks)))]
    pub fn submit_blocks(
        &mut self,
        #[serializer(borsh)] headers: Vec<Header>,
    ) -> PromiseOrValue<()> {
        self.submit_blocks_aux(headers.into_iter().map(|h| (h, None::<AuxData>)).collect())
    }

    #[payable]
    #[pause(except(roles(Role::UnrestrictedSubmitBlocks)))]
    pub fn submit_blocks_aux(
        &mut self,
        #[serializer(borsh)] headers: Vec<(Header, Option<AuxData>)>,
    ) -> PromiseOrValue<()> {
        let amount = env::attached_deposit();
        let initial_storage = env::storage_usage();
        let num_of_headers = headers.len().try_into().unwrap();

        for header in headers {
            self.submit_block_header(header.0, header.1);
        }

        self.run_mainchain_gc(num_of_headers);
        let diff_storage_usage = env::storage_usage().saturating_sub(initial_storage);
        let required_deposit = env::storage_byte_cost().saturating_mul(diff_storage_usage.into());

        require!(
            amount >= required_deposit,
            format!("Required deposit {}", required_deposit)
        );

        let refund = amount.saturating_sub(required_deposit);
        if refund > NearToken::from_near(0) {
            Promise::new(env::predecessor_account_id())
                .transfer(refund)
                .into()
        } else {
            PromiseOrValue::Value(())
        }
    }

    pub fn get_last_block_header(&self) -> ExtendedHeader {
        self.headers_pool
            .get(&self.mainchain_tip_blockhash)
            .unwrap_or_else(|| env::panic_str(ERR_KEY_NOT_EXIST))
    }

    pub fn get_block_hash_by_height(&self, height: u64) -> Option<H256> {
        self.mainchain_height_to_header.get(&height)
    }

    #[allow(clippy::needless_pass_by_value)]
    pub fn get_height_by_block_hash(&self, blockhash: H256) -> Option<u64> {
        self.mainchain_header_to_height.get(&blockhash)
    }

    pub fn get_mainchain_size(&self) -> u64 {
        let tail = self
            .headers_pool
            .get(&self.mainchain_initial_blockhash)
            .unwrap_or_else(|| env::panic_str(ERR_KEY_NOT_EXIST));
        let tip = self
            .headers_pool
            .get(&self.mainchain_tip_blockhash)
            .unwrap_or_else(|| env::panic_str(ERR_KEY_NOT_EXIST));
        tip.block_height - tail.block_height + 1
    }

    /// This method return n last blocks from the mainchain
    /// # Panics
    /// Cannot find a tip of main chain in a pool
    pub fn get_last_n_blocks_hashes(&self, skip: u64, limit: u64) -> Vec<H256> {
        let mut block_hashes = vec![];
        let tip_hash = &self.mainchain_tip_blockhash;
        let tip = self
            .headers_pool
            .get(tip_hash)
            .unwrap_or_else(|| env::panic_str("heaviest block should be recorded"));

        let min_block_height = self
            .headers_pool
            .get(&self.mainchain_initial_blockhash)
            .unwrap_or_else(|| env::panic_str("initial block should be recorded"))
            .block_height;

        let start_block_height =
            std::cmp::max(min_block_height, tip.block_height - limit - skip + 1);

        for height in start_block_height..=(tip.block_height - skip) {
            if let Some(block_hash) = self.mainchain_height_to_header.get(&height) {
                block_hashes.push(block_hash);
            }
        }

        block_hashes
    }

    /// Verifies that a transaction is included in a block at a given block height
    ///
    /// @param tx_id transaction identifier
    /// @param tx_block_blockhash block hash at which transacton is supposedly included
    /// @param tx_index index of transaction in the block's tx merkle tree
    /// @param merkle_proof  merkle tree path (concatenated LE sha256 hashes) (does not contain initial transaction_hash and merkle_root)
    /// @param confirmations how many confirmed blocks we want to have before the transaction is valid
    /// @return True if tx_id is at the claimed position in the block at the given blockhash, False otherwise
    ///
    /// # Warning
    /// This function may return `true` if the provided `tx_id` is a hash of an internal node in the Merkle tree rather than a valid transaction hash.
    /// We assume that validation of whether the `tx_id` corresponds to a valid transaction hash is performed at a higher level of verification.
    ///
    /// # Panics
    /// Multiple cases
    #[pause]
    pub fn verify_transaction_inclusion(&self, #[serializer(borsh)] args: ProofArgs) -> bool {
        require!(
            args.confirmations <= self.gc_threshold,
            "The required number of confirmations exceeds the number of blocks stored in memory"
        );

        let heaviest_block_header = self
            .headers_pool
            .get(&self.mainchain_tip_blockhash)
            .unwrap_or_else(|| env::panic_str(ERR_KEY_NOT_EXIST));
        let target_block_height = self
            .mainchain_header_to_height
            .get(&args.tx_block_blockhash)
            .unwrap_or_else(|| env::panic_str("block does not belong to the current main chain"));

        // Check requested confirmations. No need to compute proof if insufficient confirmations.
        require!(
            (heaviest_block_header.block_height).saturating_sub(target_block_height) + 1
                >= args.confirmations,
            "Not enough blocks confirmed"
        );

        let header = self
            .headers_pool
            .get(&args.tx_block_blockhash)
            .unwrap_or_else(|| env::panic_str("cannot find requested transaction block"));

        // compute merkle tree root and check if it matches block's original merkle tree root
        merkle_tools::compute_root_from_merkle_proof(
            args.tx_id,
            usize::try_from(args.tx_index).unwrap(),
            &args.merkle_proof,
        ) == header.block_header.merkle_root
    }

    /// Public call to run GC on a mainchain.
    /// batch_size is how many block headers should be removed in the execution
    ///
    /// # Panics
    /// If initial blockheader or tip blockheader are not in a header pool
    #[pause(except(roles(Role::UnrestrictedRunGC)))]
    pub fn run_mainchain_gc(&mut self, batch_size: u64) {
        let initial_blockheader = self
            .headers_pool
            .get(&self.mainchain_initial_blockhash)
            .unwrap_or_else(|| env::panic_str(ERR_KEY_NOT_EXIST));

        let tip_blockheader = self
            .headers_pool
            .get(&self.mainchain_tip_blockhash)
            .unwrap_or_else(|| env::panic_str(ERR_KEY_NOT_EXIST));

        let amount_of_headers_we_store =
            tip_blockheader.block_height - initial_blockheader.block_height + 1;

        if amount_of_headers_we_store > self.gc_threshold {
            let total_amount_to_remove = amount_of_headers_we_store - self.gc_threshold;
            let selected_amount_to_remove = std::cmp::min(total_amount_to_remove, batch_size);

            let start_removal_height = initial_blockheader.block_height;
            let end_removal_height = initial_blockheader.block_height + selected_amount_to_remove;
            env::log_str(&format!(
                "Num of blocks to remove {selected_amount_to_remove}"
            ));

            for height in start_removal_height..end_removal_height {
                let blockhash = &self
                    .mainchain_height_to_header
                    .get(&height)
                    .unwrap_or_else(|| env::panic_str(ERR_KEY_NOT_EXIST));

                self.remove_block_header(blockhash);
                self.mainchain_height_to_header.remove(&height);
            }

            self.mainchain_initial_blockhash = self
                .mainchain_height_to_header
                .get(&end_removal_height)
                .unwrap_or_else(|| env::panic_str(ERR_KEY_NOT_EXIST));
        }
    }

    pub fn get_config() -> NetworkConfig {
        NetworkConfig::new(&Self::get_network())
    }

    pub fn get_network() -> Network {
        #[cfg(feature = "bitcoin")]
        {
            Network::Bitcoin
        }
        #[cfg(feature = "bitcoin_testnet")]
        {
            Network::BitcoinTestnet
        }
        #[cfg(feature = "litecoin")]
        {
            Network::Litecoin
        }
        #[cfg(feature = "litecoin_testnet")]
        {
            Network::LitecoinTestnet
        }
        #[cfg(feature = "dogecoin")]
        {
            Network::Dogecoin
        }
        #[cfg(feature = "dogecoin_testnet")]
        {
            Network::DogecoinTestnet
        }
    }
}

impl BtcLightClient {
    fn init_genesis(&mut self, block_header: Header, block_hash: &H256, block_height: u64) {
        let config = Self::get_config();

        env::log_str(&format!(
            "Init with block hash {block_hash} at height {block_height}"
        ));

        require!(block_height % config.blocks_per_adjustment == 0, format!("Error: The initial block height must be divisible by {} to ensure proper alignment with difficulty adjustment periods.", config.blocks_per_adjustment));

        let current_block_hash = block_header.block_hash();
        require!(&current_block_hash == block_hash, "Invalid block hash");
        let chain_work = block_header.work();

        let header = ExtendedHeader {
            block_header,
            block_height,
            block_hash: current_block_hash.clone(),
            chain_work,
            aux_parent_block: None,
        };

        self.store_block_header(&header);
        self.mainchain_initial_blockhash
            .clone_from(&current_block_hash);
        self.mainchain_tip_blockhash = current_block_hash;
    }

    fn submit_block_header(&mut self, block_header: Header, aux_data: Option<AuxData>) {
        // We do not have a previous block in the headers_pool, there is a high probability
        // it means we are starting to receive a new fork,
        // so what we do now is we are returning the error code
        // to ask the relay to deploy the previous block.
        //
        // Offchain relay now, should submit blocks one by one in decreasing height order
        // 80 -> 79 -> 78 -> ...
        // And do it until we can accept the block.
        // It means we found an initial fork position.
        // We are starting to gather new fork from this initial position.
        let prev_block_header = self
            .headers_pool
            .get(&block_header.prev_block_hash)
            .unwrap_or_else(|| env::panic_str("PrevBlockNotFound"));

        self.check_target(&block_header, &prev_block_header);

        let current_block_hash = block_header.block_hash();

        let aux_parent_block = match aux_data {
            None => {
                let pow_hash = block_header.block_hash_pow();
                require!(
                    self.skip_pow_verification
                        || U256::from_le_bytes(&pow_hash.0) <= block_header.target(),
                    format!("block should have correct pow")
                );
                None
            }
            Some(aux_data) => {
                self.check_aux(&block_header, &aux_data);
                Some(aux_data.parent_block.block_hash())
            }
        };

        let (current_block_computed_chain_work, overflow) = prev_block_header
            .chain_work
            .overflowing_add(block_header.work());
        require!(!overflow, "Addition of U256 values overflowed");

        let current_header = ExtendedHeader {
            block_header,
            block_hash: current_block_hash,
            chain_work: current_block_computed_chain_work,
            block_height: 1 + prev_block_header.block_height,
            aux_parent_block,
        };

        // Main chain submission
        if prev_block_header.block_hash == self.mainchain_tip_blockhash {
            // Probably we should check if it is not in a mainchain?
            // chainwork > highScore
            log!("Block {}: saving to mainchain", current_header.block_hash);
            // Validate chain
            assert_eq!(
                self.mainchain_tip_blockhash,
                current_header.block_header.prev_block_hash
            );

            self.store_block_header(&current_header);
            self.mainchain_tip_blockhash = current_header.block_hash;
        } else {
            log!("Block {}: saving to fork", current_header.block_hash);
            // Fork submission
            let main_chain_tip_header = self
                .headers_pool
                .get(&self.mainchain_tip_blockhash)
                .unwrap_or_else(|| env::panic_str("tip should be in a header pool"));

            let last_main_chain_block_height = main_chain_tip_header.block_height;
            let total_main_chain_chainwork = main_chain_tip_header.chain_work;

            self.store_fork_header(&current_header);

            // Current chainwork is higher than on a current mainchain, let's promote the fork
            if current_block_computed_chain_work > total_main_chain_chainwork {
                log!("Chain reorg");
                self.reorg_chain(current_header, last_main_chain_block_height);
            }
        }
    }

    fn check_aux(&mut self, block_header: &Header, aux_data: &AuxData) {
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
            self.skip_pow_verification || U256::from_le_bytes(&pow_hash.0) <= block_header.target(),
            format!("block should have correct pow")
        );
    }

    fn check_target_testnet(
        &self,
        block_header: &Header,
        prev_block_header: &ExtendedHeader,
        config: NetworkConfig,
    ) {
        let time_diff = block_header
            .time
            .saturating_sub(prev_block_header.block_header.time);
        if time_diff >= 2 * config.pow_target_time_between_blocks_secs {
            require!(
                block_header.bits == config.proof_of_work_limit_bits,
                format!(
                    "Error: Incorrect bits. Expected bits: {}; Actual bits: {}",
                    config.proof_of_work_limit_bits, block_header.bits
                )
            );
        } else {
            let mut current_block_header = prev_block_header.clone();
            while current_block_header.block_header.bits == config.proof_of_work_limit_bits
                && current_block_header.block_height % config.blocks_per_adjustment != 0
            {
                current_block_header = self
                    .headers_pool
                    .get(&current_block_header.block_header.prev_block_hash)
                    .unwrap_or_else(|| env::panic_str(ERR_KEY_NOT_EXIST));
            }

            let last_bits = current_block_header.block_header.bits;
            require!(
                last_bits == block_header.bits,
                format!(
                    "Error: Incorrect bits. Expected bits: {}; Actual bits: {}",
                    last_bits, block_header.bits
                )
            );
        }
    }

    fn check_target(&self, block_header: &Header, prev_block_header: &ExtendedHeader) {
        let config = Self::get_config();

        if (prev_block_header.block_height + 1) % config.blocks_per_adjustment != 0 {
            if config.pow_allow_min_difficulty_blocks {
                return self.check_target_testnet(block_header, prev_block_header, config);
            }
            require!(
                block_header.bits == prev_block_header.block_header.bits,
                format!(
                    "Error: Incorrect bits. Expected bits: {}; Actual bits: {}.",
                    prev_block_header.block_header.bits, block_header.bits
                )
            );
            return;
        }

        #[cfg(not(any(feature = "dogecoin", feature = "dogecoin_testnet")))]
        let first_block_height = prev_block_header.block_height + 1 - config.blocks_per_adjustment;

        #[cfg(any(feature = "dogecoin", feature = "dogecoin_testnet"))]
        let first_block_height = prev_block_header.block_height - config.blocks_per_adjustment;

        let interval_tail_header_hash =
            match self.mainchain_height_to_header.get(&first_block_height) {
                None => return,
                Some(header_hash) => header_hash,
            };

        let interval_tail_extend_header = self
            .headers_pool
            .get(&interval_tail_header_hash)
            .unwrap_or_else(|| env::panic_str(ERR_KEY_NOT_EXIST));
        let prev_block_time = prev_block_header.block_header.time;

        let actual_time_taken: i64 =
            (prev_block_time as i64) - (interval_tail_extend_header.block_header.time as i64);
        let modulated_time = Self::get_modulated_time(actual_time_taken);

        let last_target = prev_block_header.block_header.target();

        let (mut new_target, new_target_overflow) = last_target.overflowing_mul(modulated_time);
        require!(!new_target_overflow, "new target overflow");
        new_target = new_target / U256::from(config.expected_time_secs);

        if new_target > config.pow_limt {
            new_target = config.pow_limt;
        }

        #[cfg(any(feature = "dogecoin_testnet"))]
        if (block_header.time as u64) > (prev_block_time as u64) + config.expected_time_secs * 2 {
            new_target = config.pow_limt;
        }

        let expected_bits = new_target.target_to_bits();

        require!(
            expected_bits == block_header.bits,
            format!(
                "Error: Incorrect target. Expected bits: {:?}, Actual bits: {:?}",
                expected_bits, block_header.bits
            )
        );
    }

    #[cfg(not(any(feature = "dogecoin", feature = "dogecoin_testnet")))]
    fn get_modulated_time(actual_time_taken: i64) -> u64 {
        use btc_types::header::MAX_ADJUSTMENT_FACTOR;

        let config = Self::get_config();
        let mut modulated_time: u64 = u64::try_from(actual_time_taken).unwrap_or(0);

        if modulated_time < config.expected_time_secs / MAX_ADJUSTMENT_FACTOR {
            modulated_time = config.expected_time_secs / MAX_ADJUSTMENT_FACTOR;
        }
        if modulated_time > config.expected_time_secs * MAX_ADJUSTMENT_FACTOR {
            modulated_time = config.expected_time_secs * MAX_ADJUSTMENT_FACTOR;
        }

        modulated_time
    }

    #[cfg(any(feature = "dogecoin", feature = "dogecoin_testnet"))]
    fn get_modulated_time(actual_time_taken: i64) -> u64 {
        let config = Self::get_config();

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

    /// The most expensive operation which reorganizes the chain, based on fork weight
    fn reorg_chain(&mut self, fork_tip_header: ExtendedHeader, last_main_chain_block_height: u64) {
        let fork_tip_height = fork_tip_header.block_height;
        if last_main_chain_block_height > fork_tip_height {
            // If we see that main chain is longer than fork we first garbage collect
            // outstanding main chain blocks:
            //
            //      [m1] - [m2] - [m3] - [m4] <- We should remove [m4]
            //     /
            // [m0]
            //     \
            //      [f1] - [f2] - [f3]
            for height in (fork_tip_height + 1)..=last_main_chain_block_height {
                let current_main_chain_blockhash = self
                    .mainchain_height_to_header
                    .get(&height)
                    .unwrap_or_else(|| env::panic_str("cannot get a block"));
                self.remove_block_header(&current_main_chain_blockhash);
                self.mainchain_height_to_header.remove(&height);
            }
        }

        // Now we are in a situation where mainchain is equivalent to fork size:
        //
        //      [m1] - [m2] - [m3] - [m4] <- main tip
        //     /
        // [m0]
        //     \
        //      [f1] - [f2] - [f3] - [f4] <- fork tip
        //
        //
        // Or in a situation where it is shorter:
        //
        //      [m1] - [m2] - [m3] <- main tip
        //     /
        // [m0]
        //     \
        //      [f1] - [f2] - [f3] - [f4] <- fork tip

        let fork_tip_hash = fork_tip_header.block_hash.clone();
        let mut fork_header_cursor = fork_tip_header;

        while !self
            .mainchain_header_to_height
            .contains_key(&fork_header_cursor.block_hash)
        {
            let prev_block_hash = fork_header_cursor.block_header.prev_block_hash;
            let current_block_hash = fork_header_cursor.block_hash;
            let current_height = fork_header_cursor.block_height;

            // Inserting the fork block into the main chain, if some mainchain block is occupying
            // this height let's save its hashcode
            let main_chain_block = self
                .mainchain_height_to_header
                .insert(&current_height, &current_block_hash);
            self.mainchain_header_to_height
                .insert(&current_block_hash, &current_height);

            // If we found a mainchain block at the current height than remove this block from the
            // header pool and from the header -> height map
            if let Some(current_main_chain_blockhash) = main_chain_block {
                self.remove_block_header(&current_main_chain_blockhash);
            }

            // Switch iterator cursor to the previous block in fork
            fork_header_cursor = self
                .headers_pool
                .get(&prev_block_hash)
                .unwrap_or_else(|| env::panic_str("previous fork block should be there"));
        }

        // Updating tip of the new main chain
        self.mainchain_tip_blockhash = fork_tip_hash;
    }

    /// Stores parsed block header and meta information
    fn store_block_header(&mut self, header: &ExtendedHeader) {
        self.mainchain_height_to_header
            .insert(&header.block_height, &header.block_hash);
        self.mainchain_header_to_height
            .insert(&header.block_hash, &header.block_height);
        self.headers_pool.insert(&header.block_hash, header);
    }

    /// Remove block header and meta information
    fn remove_block_header(&mut self, header_block_hash: &H256) {
        self.mainchain_header_to_height.remove(header_block_hash);
        if let Some(header) = self.headers_pool.remove(header_block_hash) {
            if let Some(aux_parent_blockhash) = header.aux_parent_block {
                self.used_aux_parent_blocks.remove(&aux_parent_blockhash);
            }
        }
    }

    /// Stores and handles fork submissions
    fn store_fork_header(&mut self, header: &ExtendedHeader) {
        self.headers_pool.insert(&header.block_hash, header);
    }
}

/*
 * The rest of this file holds the inline tests for the code above
 * Learn more about Rust tests: https://doc.rust-lang.org/book/ch11-01-writing-tests.html
 */
#[cfg(test)]
mod tests {
    use super::*;

    fn decode_hex(hex: &str) -> H256 {
        hex.parse().unwrap()
    }

    fn genesis_block_header() -> Header {
        let json_value = serde_json::json!({
            "version": 1,
            "prev_block_hash": "0000000000000000000000000000000000000000000000000000000000000000",
            "merkle_root": "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b",
            "time": 1_231_006_505,
            "bits": 486_604_799,
            "nonce": 2_083_236_893
        });

        serde_json::from_value(json_value).expect("value is invalid")
    }

    // Bitcoin header example
    fn block_header_example() -> Header {
        let json_value = serde_json::json!({
            // block_hash: 62703463e75c025987093c6fa96e7261ac982063ea048a0550407ddbbe865345
            "version": 1,
            "prev_block_hash": "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f",
            "merkle_root": "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b",
            "time": 1_231_006_506,
            "bits": 486_604_799,
            "nonce": 2_083_236_893
        });

        serde_json::from_value(json_value).expect("value is invalid")
    }

    fn fork_block_header_example() -> Header {
        let json_value = serde_json::json!({
            // "hash": "00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048",
            //"chainwork": "0000000000000000000000000000000000000000000000000000000200020002",
            "version": 1,
            "merkle_root": "0e3e2357e806b6cdb1f70b54c3a3a17b6714ee1f0e68bebb44a74b1efd512098",
            "time": 1_231_469_665,
            "nonce": 2_573_394_689_u32,
            "bits": 486_604_799,
            "prev_block_hash": "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f",
        });

        serde_json::from_value(json_value).expect("value is invalid")
    }

    fn fork_block_header_example_2() -> Header {
        let json_value = serde_json::json!({
            // "hash": "000000006a625f06636b8bb6ac7b960a8d03705d1ace08b1a19da3fdcc99ddbd",
            // "chainwork": "0000000000000000000000000000000000000000000000000000000300030003",
          "version": 1,
          "merkle_root": "9b0fc92260312ce44e74ef369f5c66bbb85848f2eddd5a7a1cde251e54ccfdd5",
          "time": 1_231_469_744,
          "nonce": 1_639_830_024,
          "bits": 486_604_799,
          "prev_block_hash": "00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048",
        });

        serde_json::from_value(json_value).expect("value is invalid")
    }

    fn get_default_init_args() -> InitArgs {
        let genesis_block = genesis_block_header();
        InitArgs {
            genesis_block_hash: genesis_block.block_hash(),
            genesis_block,
            genesis_block_height: 0,
            skip_pow_verification: false,
            gc_threshold: 3,
        }
    }

    fn get_default_init_args_with_skip_pow() -> InitArgs {
        let genesis_block = genesis_block_header();
        InitArgs {
            genesis_block_hash: genesis_block.block_hash(),
            genesis_block,
            genesis_block_height: 0,
            skip_pow_verification: true,
            gc_threshold: 3,
        }
    }

    #[test]
    #[should_panic(expected = "block should have correct pow")]
    fn test_pow_validator_works_correctly_for_wrong_block() {
        let header = block_header_example();

        let mut contract = BtcLightClient::init(get_default_init_args());

        contract.submit_block_header(header, None);
    }

    #[test]
    fn test_pow_validator_works_correctly_for_correct_block() {
        let header = fork_block_header_example();
        let mut contract = BtcLightClient::init(get_default_init_args());

        contract.submit_block_header(header.clone(), None);

        let received_header = contract.get_last_block_header();

        assert_eq!(
            received_header,
            ExtendedHeader {
                block_header: header,
                block_hash: decode_hex(
                    "00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048"
                ),
                chain_work: U256::from_be_bytes(&[
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 2, 0, 2, 0, 2
                ]),
                block_height: 1,
                aux_parent_block: None,
            }
        );
    }

    #[test]
    fn test_saving_mainchain_block_header() {
        let header = block_header_example();

        let mut contract = BtcLightClient::init(get_default_init_args_with_skip_pow());

        contract.submit_block_header(header.clone(), None);

        let received_header = contract.get_last_block_header();

        assert_eq!(
            received_header,
            ExtendedHeader {
                block_header: header,
                block_hash: decode_hex(
                    "62703463e75c025987093c6fa96e7261ac982063ea048a0550407ddbbe865345"
                ),
                chain_work: U256::from_be_bytes(&[
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 2, 0, 2, 0, 2
                ]),
                block_height: 1,
                aux_parent_block: None,
            }
        );
    }

    #[test]
    fn test_submitting_new_fork_block_header() {
        let header = block_header_example();

        let mut contract = BtcLightClient::init(get_default_init_args_with_skip_pow());

        contract.submit_block_header(header.clone(), None);

        contract.submit_block_header(fork_block_header_example(), None);

        let received_header = contract.get_last_block_header();

        assert_eq!(
            received_header,
            ExtendedHeader {
                block_header: header,
                block_hash: decode_hex(
                    "62703463e75c025987093c6fa96e7261ac982063ea048a0550407ddbbe865345"
                ),
                chain_work: U256::from_be_bytes(&[
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 2, 0, 2, 0, 2
                ]),
                block_height: 1,
                aux_parent_block: None,
            }
        );
    }

    // test we can insert a block and get block back by it's height
    #[test]
    fn test_getting_block_by_height() {
        let mut contract = BtcLightClient::init(get_default_init_args_with_skip_pow());

        contract.submit_block_header(block_header_example(), None);

        assert_eq!(
            contract.get_block_hash_by_height(0).unwrap(),
            genesis_block_header().block_hash(),
        );
        assert_eq!(
            contract.get_block_hash_by_height(1).unwrap(),
            block_header_example().block_hash()
        );
    }

    #[test]
    fn test_getting_height_by_block() {
        let mut contract = BtcLightClient::init(get_default_init_args_with_skip_pow());

        contract.submit_block_header(block_header_example(), None);

        assert_eq!(
            contract
                .get_height_by_block_hash(genesis_block_header().block_hash())
                .unwrap(),
            0
        );
        assert_eq!(
            contract
                .get_height_by_block_hash(block_header_example().block_hash())
                .unwrap(),
            1
        );
    }

    #[test]
    fn test_submitting_existing_fork_block_header_and_promote_fork() {
        let mut contract = BtcLightClient::init(get_default_init_args_with_skip_pow());

        contract.submit_block_header(block_header_example(), None);

        contract.submit_block_header(fork_block_header_example(), None);
        contract.submit_block_header(fork_block_header_example_2(), None);

        let received_header = contract.get_last_block_header();

        assert_eq!(
            received_header,
            ExtendedHeader {
                block_header: fork_block_header_example_2(),
                block_hash: decode_hex(
                    "000000006a625f06636b8bb6ac7b960a8d03705d1ace08b1a19da3fdcc99ddbd"
                ),
                chain_work: U256::from_be_bytes(&[
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 3, 0, 3, 0, 3
                ]),
                block_height: 2,
                aux_parent_block: None,
            }
        );
    }

    #[test]
    #[should_panic(expected = "Error: Incorrect bits")]
    fn test_submitting_block_with_incorrect_bits_same_period() {
        let mut contract = BtcLightClient::init(get_default_init_args_with_skip_pow());
        let mut next_header = block_header_example();
        next_header.bits += 1;
        contract.submit_block_header(next_header, None);
    }

    #[test]
    #[should_panic(expected = "PrevBlockNotFound")]
    fn test_getting_an_error_if_submitting_unattached_block() {
        let mut contract = BtcLightClient::init(get_default_init_args_with_skip_pow());

        contract.submit_block_header(fork_block_header_example_2(), None);
    }
}
