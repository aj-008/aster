use crate::{config::CacheConfig, error::AsterError, prefetchers::stream_buffer::StreamBuffer};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_prefetch_settings, default_repl_settings};

    fn cfg(prefetcher: Option<&str>) -> CacheConfig {
        CacheConfig {
            block_size: 64,
            cache_size: 32768,
            associativity: 8,
            replacement_policy: "lru".to_string(),
            prefetcher: prefetcher.map(str::to_string),
            repl_settings: default_repl_settings(),
            prefetch_settings: default_prefetch_settings(),
        }
    }

    #[test]
    fn no_prefetcher_configured_returns_none() {
        let result = prefetcher_init(&cfg(None)).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn stream_buffer_name_builds_a_prefetcher() {
        let result = prefetcher_init(&cfg(Some("stream_buffer"))).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn unknown_prefetcher_name_is_an_error() {
        match prefetcher_init(&cfg(Some("not_a_real_prefetcher"))) {
            Err(e) => assert_eq!(e.kind(), crate::error::ErrorKind::Config),
            Ok(_) => panic!("expected an error for an unrecognized prefetcher name"),
        }
    }
}
