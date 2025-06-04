use crate::{BtcLightClient, BtcLightClientExt};
use near_sdk::near;

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
}
