use btc_types::header::ExtendedHeader;

pub trait BlocksGetter {
    fn get_prev_header(&self, current_header: &ExtendedHeader) -> ExtendedHeader;
    fn get_header_by_height(&self, height: u64) -> ExtendedHeader;
}
