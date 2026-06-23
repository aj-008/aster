// i want this file to contain the structs for the cache memory hierarchy
// this means that there should be L1 L2 and L3 caches that hold things
// like accesses and hits/misses per cache
//
// each level (or just one cache struct of which 3 will be instantiated)
// will have an 'access' function that can be called when an access is 
// made (found in trace_reader.rs and called from there)
//
// this function will have to check vaild lines and see if there is a hit
// or a miss, and whether to evict or not, calling a set framework
// to perform an eviction
//
// the policy will first be implemented here, but can be standardized and
// moved later. 
//
//
// there may need to be some structure to represent a memory access,
// like the tag / offset bits for id that can be passed down to
// an eviction policy
//

use crate::policy::{ReplacementPolicy, make_policy};



// Cache
//
// since each cache is comprised of sets and each set has lines
// the struct will contain a vec of sets determined by the 
// entry in the config with each set being comprised of lines
pub struct Cache {
    block_size: usize,
    associativity: usize,
    num_sets: usize,
    policy: Box<dyn ReplacementPolicy>,

    accesses: usize,
    hits: usize,
    misses: usize,

    sets: Vec<CacheSet>,
} 

pub struct CacheSet {
    lines: Vec<CacheLine>,
}

pub struct CacheLine {
    valid: bool,
    tag: u64,

}

#[derive(PartialEq)]
pub enum AccessResult {
    Hit,
    Miss,
}

impl Cache {
    pub fn new(block_size: usize, associativity: usize, cache_size: usize, 
        policy_name: &str) -> Self {
        assert!(block_size.is_power_of_two(), "block_size must be a power of two");
        assert!(associativity.is_power_of_two(), "associativity must be a power of two");
        assert!(cache_size.is_power_of_two(), "cache_size must be a power of two");


        let num_sets = cache_size / (block_size * associativity);
        assert!(num_sets.is_power_of_two(), "num_sets must be a power of two");
        
        let sets = (0..num_sets)
            .map(|_| CacheSet {
                lines: (0..associativity)
                    .map(|_| CacheLine {
                        valid: false,
                        tag: 0,
                    })
                    .collect(),
            })
            .collect();

        let policy = make_policy(policy_name, num_sets, associativity);

        Self {
            block_size,
            associativity,
            num_sets,
            accesses: 0,
            hits: 0,
            misses: 0,
            sets,
            policy,
        }
    }

    pub fn get_hits(&self) -> usize {
        self.hits
    }
    
    pub fn get_misses(&self) -> usize {
        self.misses
    }


    pub fn access(&mut self, addr: u64) -> AccessResult {
        self.accesses += 1;
        let offset_bits = self.block_size.ilog2() as usize;
        let index_bits = self.num_sets.ilog2() as usize;
        let set_index = ((addr >> offset_bits) & (self.num_sets as u64 - 1)) as usize;
        let tag = addr >> (offset_bits + index_bits);

        // find the set being accessed,
        // look through the lines for validity and match tag
        let set = &mut self.sets[set_index];
        for line in set.lines.iter() {
            if line.valid && line.tag == tag {
                self.hits += 1;
                return AccessResult::Hit;
            }
        }

        // if there is a miss, set an invalid line to the line or evict
        self.misses += 1;
        let victim = set.lines.iter().position(|l| !l.valid)
            .unwrap_or(self.policy.find_victim(set_index, self.associativity));

        set.lines[victim] = CacheLine { valid: true, tag };

        AccessResult::Miss
    }
}




#[cfg(test)]
mod tests {
    use super::*;   

    #[test]
    fn cold_miss() {
        let mut cache = Cache::new(64, 4, 1024, "lru");

        cache.access(0x1000);
        assert_eq!(cache.misses, 1);
        assert_eq!(cache.hits, 0);
    }
    
    #[test]
    fn hit_second_access() {
        let mut cache = Cache::new(64, 4, 1024, "lru");

        cache.access(0x1000);
        cache.access(0x1000);
        assert_eq!(cache.misses, 1);
        assert_eq!(cache.hits, 1);
    }

    #[test]
    fn different_block_same_set() {
        let mut cache = Cache::new(64, 4, 1024, "lru");

        cache.access(0x400);
        cache.access(0x800);

        assert_eq!(cache.misses, 2);
        assert_eq!(cache.hits, 0);

        cache.access(0x400);
        cache.access(0x800);
        
        assert_eq!(cache.hits, 2);
    }
}
