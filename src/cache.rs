//! Cache and memory access interface
//!
//! Handles trace memory accesses and calls to cache stats updates

use crate::{
    config::{CacheConfig, Config}, error::AsterError, policy::{ReplacementPolicy, make_policy}, stats::CacheStats, trace_reader::MemAccess, prefetch::{Prefetcher, prefetcher_init},
};

/// L1D -> L2 -> LLC lookup chain driven by a single memory access stream;
/// each level is only queried on a miss in the level above it. Instruction
/// cache (`l1i`) is configured but not wired into `access`.
pub struct CacheHierarchy {
    pub l1i: Cache,
    pub l1d: Cache,
    pub l2: Cache,
    pub llc: Cache,
}

impl CacheHierarchy {
    /// Builds an L1D/L2/LLC hierarchy from `config`.
    ///
    /// # Panics
    /// Panics if any cache's geometry is invalid (see [`Cache::new`]).
    pub fn new(config: Config) -> Result<Self, AsterError> {
        let l1i = Cache::new(&config.l1i)?;
        let l1d = Cache::new(&config.l1d)?;
        let l2 = Cache::new(&config.l2)?;
        let llc = Cache::new(&config.llc)?;
        Ok(Self { l1i, l1d, l2, llc })
    }

    pub fn access_instruction(&mut self, ip: u64) {
        let mut access = MemAccess { addr: ip, pc: ip, is_write: false, hit: None };
        if self.l1i.access(&mut access).result == AccessResult::Miss
            && self.l2.access(&mut access).result == AccessResult::Miss
        {
            self.llc.access(&mut access);
        }
    }

    pub fn access_data(&mut self, access: &mut MemAccess) {
        let l1_outcome = self.l1d.access(access);

        if let Some(addr) = l1_outcome.writeback_addr &&
            let Some(further) = writeback_into(&mut self.l2, addr) {
                writeback_into(&mut self.llc, further);
        }

        if l1_outcome.result == AccessResult::Miss {
            let l2_outcome = self.l2.access(access);
            if let Some(addr) = l2_outcome.writeback_addr {
                writeback_into(&mut self.llc, addr);
            }

            if l2_outcome.result == AccessResult::Miss {
                self.llc.access(access);
            }
        }
    }

    /// Resets hit/miss counters on every level (used after warmup).
    pub fn reset_stats(&mut self) {
        self.l1d.reset_stats();
        self.l2.reset_stats();
        self.llc.reset_stats();
    }
}

fn writeback_into(target: &mut Cache, addr: u64) -> Option<u64> {
    target.writeback(addr).writeback_addr
}

/// Cache object
pub struct Cache {
    pub block_size: usize,
    pub associativity: usize,
    pub num_sets: usize,
    policy: Box<dyn ReplacementPolicy>,
    prefetcher: Option<Box<dyn Prefetcher>>,

    accesses: usize,
    hits: usize,
    misses: usize,

    sets: Vec<CacheSet>,
    writebacks: usize,
    writebacks_received: usize,

    prefetch_hits: usize,
    prefetch_unused: usize, 
}

/// CacheSet represents one set in a cache composed of CacheLines
pub struct CacheSet {
    lines: Vec<CacheLine>,
}

/// CacheLine represents one line in a set
#[derive(Clone, Copy)]
pub struct CacheLine {
    valid: bool,
    dirty: bool,
    prefetched: bool,
    tag: u64,
}

/// Result type of a memory access
#[derive(PartialEq, Debug)]
pub enum AccessResult {
    Hit,
    Miss,
}


#[derive(PartialEq, Debug)]
pub enum AccessKind {
    Demand,
    Writeback,
    Prefetch,
}



pub struct AccessOutcome {
    pub result: AccessResult,
    pub writeback_addr: Option<u64>,
    pub prefetch_addrs: Option<Vec<u64>>,
}

impl Cache {
    /// Instantiates and returns a cache object
    ///
    /// # Panics
    /// Panics if `cache_size / (block_size * associativity)` is not a
    /// power of two (includes the case where it evaluates to zero because
    /// `block_size * associativity > cache_size`).
    pub fn new(config: &CacheConfig) -> Result<Self, AsterError> {
        let block_size = config.block_size;
        let associativity = config.associativity;
        let cache_size = config.cache_size;

        let num_sets = cache_size / (block_size * associativity);
        assert!(
            num_sets.is_power_of_two(),
            "num_sets must be a power of two"
        );

        let sets = (0..num_sets)
            .map(|_| CacheSet {
                lines: (0..associativity)
                    .map(|_| CacheLine {
                        valid: false,
                        dirty: false,
                        prefetched: false,
                        tag: 0,
                    })
                    .collect(),
            })
            .collect();

        let policy = make_policy(config)?;

        let prefetcher = prefetcher_init(config)?;

        Ok(Self {
            block_size,
            associativity,
            num_sets,
            accesses: 0,
            hits: 0,
            misses: 0,
            sets,
            policy,
            prefetcher,
            writebacks: 0,
            writebacks_received: 0,
            prefetch_hits: 0,
            prefetch_unused: 0,
        })
    }

    /// Getter for hits
    pub fn get_hits(&self) -> usize {
        self.hits
    }

    /// Getter for misses
    pub fn get_misses(&self) -> usize {
        self.misses
    }

    pub fn access(&mut self, access: &mut MemAccess) -> AccessOutcome {
        self.do_access(access, AccessKind::Demand)
    }
    
    pub fn writeback(&mut self, addr: u64) -> AccessOutcome {
        let mut wb = MemAccess { addr, pc: 0, is_write: true, hit: None };
        self.do_access(&mut wb, AccessKind::Writeback)
    }

    pub fn install_prefetch(&mut self, addr: u64) {
        let offset_bits = self.block_size.ilog2() as usize;
        let index_bits = self.num_sets.ilog2() as usize;
        let tag = addr >> (offset_bits + index_bits);
        let set_index = ((addr >> offset_bits) & (self.num_sets as u64 - 1)) as usize;
        let already_present = self.sets[set_index]
            .lines
            .iter()
            .any(|l| l.valid && l.tag == tag);
        if already_present { return; }
        let mut pf_access = MemAccess { addr, pc: 0, is_write: false, hit: None };
        self.do_access(&mut pf_access, AccessKind::Prefetch);

    }

    /// Simulates an access on the cache object given a memory address
    fn do_access(&mut self, access: &mut MemAccess, kind: AccessKind) -> AccessOutcome
    {
        if kind == AccessKind::Demand { self.accesses += 1; }
        let offset_bits = self.block_size.ilog2() as usize;
        let index_bits = self.num_sets.ilog2() as usize;
        let set_index = ((access.addr >> offset_bits) & (self.num_sets as u64 - 1)) as usize;
        let tag = access.addr >> (offset_bits + index_bits);

        // find the set being accessed,
        // look through the lines for validity and match tag
        let hit_way = self.sets[set_index]
            .lines
            .iter()
            .enumerate()
            .find(|(_, line)| line.valid && line.tag == tag)
            .map(|(way, _)| way);

        if let Some(way) = hit_way {
        if kind == AccessKind::Demand { self.hits += 1; } else { self.writebacks_received += 1; }
            if access.is_write {
                self.sets[set_index].lines[way].dirty = true;
            }
            if self.sets[set_index].lines[way].prefetched {
                self.prefetch_hits += 1;
                self.sets[set_index].lines[way].prefetched = false;
            }
            access.hit = Some(true);
            self.policy.update(set_index, way, access);
            
            //prefetch on demand access only
            let prefetch_addrs = if kind == AccessKind::Demand {
                self.determine_prefetch_addrs(access, hit_way)
            } else {
                None
            };
            return AccessOutcome { result: AccessResult::Hit, writeback_addr: None, prefetch_addrs };
        }

        // if there is a miss, set an invalid line to the line or evict
        if kind == AccessKind::Demand { self.misses += 1; }
        access.hit = Some(false);
        let victim = self.sets[set_index]
            .lines
            .iter()
            .position(|l| !l.valid)
            .unwrap_or_else(|| self.policy.find_victim(set_index));

        let evicted = self.sets[set_index].lines[victim];
        self.sets[set_index].lines[victim] = CacheLine {
            valid: true,
            dirty: access.is_write,
            prefetched: kind == AccessKind::Prefetch,
            tag,
        };
        if evicted.prefetched {
            self.prefetch_unused += 1;
        }
        self.policy.install(set_index, victim, access);

        let writeback_addr = if evicted.valid && evicted.dirty {
            self.writebacks += 1;
            // you dont need to keep offset_bits here because we only care about tag and index
            Some((evicted.tag << (offset_bits + index_bits)) | ((set_index as u64) << offset_bits))
        } else {
            None
        };

        // can this be simplified? same code block twice in do_access fn
        let prefetch_addrs = if kind == AccessKind::Demand {
            self.determine_prefetch_addrs(access, hit_way)
        } else {
            None
        };
        AccessOutcome { result: AccessResult::Miss, writeback_addr, prefetch_addrs }
    }

    fn determine_prefetch_addrs(&mut self, access: &mut MemAccess, hit_way: Option<usize>) -> Option<Vec<u64>> {
        if let Some(pf) = &mut self.prefetcher {
            let candidates = pf.observe(access.addr, access.pc, hit_way.is_some());
            for addr in &candidates {
                self.install_prefetch(*addr);
            }
            Some(candidates)
        } else {
            None
        }
    }

    /// Returns a snapshot of this cache's hit/miss counters.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits as u64,
            misses: self.misses as u64,
        }
    }

    /// Zeroes access/hit/miss counters without touching cache/policy state.
    pub fn reset_stats(&mut self) {
        self.accesses = 0;
        self.hits = 0;
        self.misses = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CacheConfig, default_repl_settings};
    use crate::trace_reader::MemAccess;

    fn dummy_access(addr: u64) -> MemAccess {
        MemAccess {
            addr,
            pc: 0,
            is_write: false,
            hit: None,
        }
    }

    #[test]
    fn lru_second_access_hits() {
        let config = CacheConfig {
            block_size: 64,
            associativity: 4,
            cache_size: 32768,
            replacement_policy: "lru".to_string(),
            prefetcher: None,
            repl_settings: default_repl_settings(),
        };
        let mut cache = Cache::new(&config).unwrap();
        let mut a = dummy_access(0x1000);

        assert_eq!(cache.access(&mut a).result, AccessResult::Miss);
        assert_eq!(cache.access(&mut a).result, AccessResult::Hit);
    }
}

