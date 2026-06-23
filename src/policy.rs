use crate::policies::lru::Lru;


pub trait ReplacementPolicy {
    fn update(&mut self, set: usize, way: usize);
    fn find_victim(&self, set: usize, num_ways: usize) -> usize;
    fn install(&mut self, set: usize, way: usize);
}

pub fn make_policy(name: &str, num_sets: usize, associativity: usize) -> Box<dyn ReplacementPolicy> {
    match name {
        "lru" => Box::new(Lru::new(num_sets, associativity)),
        other => panic!("unknown policy: {}", other),
    }
}
