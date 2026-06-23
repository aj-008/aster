use crate::policy::ReplacementPolicy;

pub struct Lru {
    timestamps: Vec<Vec<u64>>,
    clock: u64,
}

impl Lru {
    pub fn new(num_sets: usize, associativity: usize) -> Self {
        Self {
            timestamps: vec![vec![0; associativity]; num_sets],
            clock: 0,
        }
    }
}

impl ReplacementPolicy for Lru {
    fn update(&mut self, set: usize, way: usize) {
        self.clock += 1;
        self.timestamps[set][way] = self.clock;
    }

    fn find_victim(&self, set: usize, num_ways: usize) -> usize {
        (0..num_ways)
            .min_by_key(|&w| self.timestamps[set][w])
            .unwrap()
    }

    fn install(&mut self, set: usize, way: usize) {
        self.update(set, way);
    }
}
