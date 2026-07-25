use crate::{
    config::CacheConfig,
    error::AsterError,
    prefetchers::{stream_buffer::StreamBuffer},
};

pub trait Prefetcher {
    fn observe(&mut self, addr: u64, pc: u64, hit: bool) -> Vec<u64>;
}

pub fn prefetcher_init(config: &CacheConfig) -> Result<Option<Box<dyn Prefetcher>>, AsterError> {
    match &config.prefetcher {
        Some(val) if val == &"stream_buffer".to_string() => Ok(Some(Box::new(StreamBuffer::new(
                                                                config.block_size,
                                                                &config.prefetch_settings,
                                                            )?))),
        Some(_) => Err(AsterError::Config("Prefetcher not implemented".to_string())),
        None => Ok(None),
    }
}
