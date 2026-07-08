//! Cache and memory access interface
//!
//! Handles trace memory accesses and calls to cache stats updates

use crate::{config::{Config, CacheConfig}, policy::{ReplacementPolicy, make_policy}, stats::CacheStats, trace_reader::MemAccess};


pub struct CacheHierarchy {
    pub l1d: Cache,
    pub l2: Cache,
    pub llc: Cache,
}

impl CacheHierarchy {
    pub fn new(config: Config) -> Self {
        let l1d = Cache::new(&config.l1d);
        let l2 = Cache::new(&config.l2);
        let llc = Cache::new(&config.llc);
        Self { l1d, l2, llc }
    }

    pub fn access(&mut self, access: &mut MemAccess) {
        if self.l1d.access(access) == AccessResult::Miss
            && self.l2.access(access) == AccessResult::Miss
        {
            self.llc.access(access);
        }
    }

    pub fn reset_stats(&mut self) {
        self.l1d.reset_stats();
        self.l2.reset_stats();
        self.llc.reset_stats();
    }
}

/// Cache object
pub struct Cache {
    pub block_size: usize,
    pub associativity: usize,
    pub num_sets: usize,
    policy: Box<dyn ReplacementPolicy>,

    accesses: usize,
    hits: usize,
    misses: usize,

    sets: Vec<CacheSet>,
} 

/// CacheSet represents one set in a cache composed of CacheLines
pub struct CacheSet {
    lines: Vec<CacheLine>,
}

/// CacheLine represents one line in a set
pub struct CacheLine {
    valid: bool,
    tag: u64,
}

/// Result type of a memory access
#[derive(PartialEq, Debug)]
pub enum AccessResult {
    Hit,
    Miss,
}

impl Cache {
    /// Instantiates and returns a cache object 
    pub fn new(config: &CacheConfig) -> Self {

        let block_size = config.block_size;
        let associativity = config.associativity;
        let cache_size = config.cache_size;

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

        let policy = make_policy(config).expect("uh oh, replace me with real error handling");

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

    /// Getter for hits
    pub fn get_hits(&self) -> usize {
        self.hits
    }
    
    /// Getter for misses
    pub fn get_misses(&self) -> usize {
        self.misses
    }


    /// Simulates an access on the cache object given a memory address
    pub fn access(&mut self, access: &mut MemAccess) -> AccessResult {
        self.accesses += 1;
        let offset_bits = self.block_size.ilog2() as usize;
        let index_bits = self.num_sets.ilog2() as usize;
        let set_index = ((access.addr >> offset_bits) & (self.num_sets as u64 - 1)) as usize;
        let tag = access.addr >> (offset_bits + index_bits);

        // find the set being accessed,
        // look through the lines for validity and match tag
        let hit_way = self.sets[set_index].lines
            .iter()
            .enumerate()
            .find(|(_, line)| line.valid && line.tag == tag)
            .map(|(way, _)| way);

        if let Some(way) = hit_way {
            self.hits += 1;
            access.hit = Some(true);
            self.policy.update(set_index, way, access);
            return AccessResult::Hit;
        }

        // if there is a miss, set an invalid line to the line or evict
        self.misses += 1;
        access.hit = Some(false);
        let victim = self.sets[set_index].lines
            .iter()
            .position(|l| !l.valid)
            .unwrap_or_else(|| self.policy.find_victim(set_index));

        self.sets[set_index].lines[victim] = CacheLine { valid: true, tag };
        self.policy.install(set_index, victim, access);
        AccessResult::Miss
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats { hits: self.hits as u64, misses: self.misses as u64 }
    }

    pub fn reset_stats(&mut self) {
        self.accesses = 0;
        self.hits = 0;
        self.misses = 0;
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace_reader::MemAccess;
    use crate::config::{ CacheConfig, default_repl_settings };

    fn dummy_access(addr: u64) -> MemAccess {
        MemAccess { addr, pc: 0, is_write: false, hit: None }
    }

    #[test]
    fn lru_second_access_hits() {
        let config = CacheConfig { block_size: 64, associativity: 4, cache_size: 32768, replacement_policy: "lru".to_string(), repl_settings: default_repl_settings() };
        let mut cache = Cache::new(&config);
        let mut a = dummy_access(0x1000);
        
        assert_eq!(cache.access(&mut a), AccessResult::Miss);
        assert_eq!(cache.access(&mut a), AccessResult::Hit);
    }
}


// #[cfg(test)]
// mod tests {
//    use super::*;   
//
//    #[test]
//    fn cold_miss() {
//        let mut cache = Cache::new(64, 4, 1024, "lru");
//
//        cache.access(0x1000);
//        assert_eq!(cache.misses, 1);
//        assert_eq!(cache.hits, 0);
//    }
//    
//    #[test]
//    fn hit_second_access() {
//        let mut cache = Cache::new(64, 4, 1024, "lru");
//
//        cache.access(0x1000);
//        cache.access(0x1000);
//        assert_eq!(cache.misses, 1);
//        assert_eq!(cache.hits, 1);
//    }
//
//    #[test]
//    fn different_block_same_set() {
//        let mut cache = Cache::new(64, 4, 1024, "lru");
//
//        cache.access(0x400);
//        cache.access(0x800);
//
//        assert_eq!(cache.misses, 2);
//        assert_eq!(cache.hits, 0);
//
//        cache.access(0x400);
//        cache.access(0x800);
//        
//        assert_eq!(cache.hits, 2);
//    }
//}
