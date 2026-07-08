use crate::{error::AsterError, policy::ReplacementPolicy, trace_reader::MemAccess};

pub struct Lru {
    timestamps: Vec<Vec<u64>>,
    clock: u64,
    num_ways: usize,
}

impl Lru {
    pub fn new(cache_size: usize, block_size: usize, associativity: usize, _settings: &toml::Value) -> Result<Self, AsterError> {
        // initialize timestamps so way 0 is evicted first on a cold set
        let num_sets = cache_size / (block_size * associativity);
        let timestamps = (0..num_sets)
            .map(|_| (0..associativity).map(|w| w as u64).collect())
            .collect();
        Ok(Self {
            timestamps,
            clock: associativity as u64,  // start clock above initial values
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
