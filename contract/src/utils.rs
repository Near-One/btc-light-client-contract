use btc_types::header::{ExtendedHeader, Header};

pub trait BlocksGetter {
    fn get_prev_header(&self, current_header: &Header) -> ExtendedHeader;
    fn get_header_by_height(&self, height: u64) -> ExtendedHeader;
}
