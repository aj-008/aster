use crate::{error::AsterError, policy::ReplacementPolicy, trace_reader::MemAccess};

/// Least-recently-used replacement policy. Tracks a per-way logical
/// timestamp per set and evicts the way with the oldest timestamp.
pub struct Lru {
    timestamps: Vec<Vec<u64>>,
    clock: u64,
    num_ways: usize,
}

impl Lru {
    /// Builds an `Lru` state for a cache with the given geometry.
    /// `_settings` is accepted for interface parity with other policies but
    /// LRU has no tunable settings.
    ///
    /// # Errors
    /// Never fails; returns `Result` for interface parity with
    /// [`crate::policy::make_policy`].
    pub fn new(
        cache_size: usize,
        block_size: usize,
        associativity: usize,
        _settings: &toml::Value,
    ) -> Result<Self, AsterError> {
        // initialize timestamps so way 0 is evicted first on a cold set
        let num_sets = cache_size / (block_size * associativity);
        let timestamps = (0..num_sets)
            .map(|_| (0..associativity).map(|w| w as u64).collect())
            .collect();
        Ok(Self {
            timestamps,
            clock: associativity as u64, // start clock above initial values
            num_ways: associativity,
        })
    }
}

impl ReplacementPolicy for Lru {
    fn update(&mut self, set: usize, way: usize, _access: &MemAccess) {
        self.clock += 1;
        self.timestamps[set][way] = self.clock;
    }

    fn find_victim(&mut self, set: usize) -> usize {
        (0..self.num_ways)
            .min_by_key(|&w| self.timestamps[set][w])
            .unwrap()
    }

    fn install(&mut self, set: usize, way: usize, access: &MemAccess) {
        self.update(set, way, access);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access() -> MemAccess {
        MemAccess { addr: 0, pc: 0, is_write: false, hit: None }
    }

    #[test]
    fn cold_set_evicts_way_zero_first() {
        let mut lru = Lru::new(32768, 64, 4, &toml::Value::Table(toml::map::Map::new())).unwrap();
        assert_eq!(lru.find_victim(0), 0);
    }

    #[test]
    fn evicts_least_recently_used_way() {
        let mut lru = Lru::new(32768, 64, 4, &toml::Value::Table(toml::map::Map::new())).unwrap();
        // touch every way except way 1, in order 0, 2, 3 -- way 1 remains oldest.
        lru.update(0, 0, &access());
        lru.update(0, 2, &access());
        lru.update(0, 3, &access());
        assert_eq!(lru.find_victim(0), 1);
    }

    #[test]
    fn recently_touched_way_is_not_evicted() {
        let mut lru = Lru::new(32768, 64, 2, &toml::Value::Table(toml::map::Map::new())).unwrap();
        lru.update(0, 0, &access());
        assert_eq!(lru.find_victim(0), 1, "way 1 never touched, should still be oldest");
        lru.update(0, 1, &access());
        assert_eq!(lru.find_victim(0), 0, "way 0 is now oldest");
    }

    #[test]
    fn sets_are_independent() {
        let mut lru = Lru::new(65536, 64, 2, &toml::Value::Table(toml::map::Map::new())).unwrap();
        lru.update(0, 0, &access());
        lru.update(0, 1, &access());
        // set 1 untouched: way 0 should still be its victim.
        assert_eq!(lru.find_victim(1), 0);
    }

    #[test]
    fn install_behaves_like_update() {
        let mut lru = Lru::new(32768, 64, 2, &toml::Value::Table(toml::map::Map::new())).unwrap();
        lru.install(0, 0, &access());
        assert_eq!(lru.find_victim(0), 1, "installed way 0 should no longer be the victim");
    }

    #[test]
    fn settings_value_is_ignored() {
        // Lru has no tunables; any settings value should be accepted without error.
        let mut settings = toml::map::Map::new();
        settings.insert("unused".to_string(), toml::Value::Integer(5));
        assert!(Lru::new(32768, 64, 4, &toml::Value::Table(settings)).is_ok());
    }
}
