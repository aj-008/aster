//! Cache and memory access interface
//!
//! Handles trace memory accesses and calls to cache stats updates

use crate::{
    config::{CacheConfig, Config},
    error::AsterError,
    policy::{ReplacementPolicy, make_policy},
    prefetch::{Prefetcher, prefetcher_init},
    stats::CacheStats,
    trace_reader::MemAccess,
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
        let mut access = MemAccess {
            addr: ip,
            pc: ip,
            is_write: false,
            hit: None,
        };
        if self.l1i.access(&mut access).result == AccessResult::Miss
            && self.l2.access(&mut access).result == AccessResult::Miss
        {
            self.llc.access(&mut access);
        }
    }

    pub fn access_data(&mut self, access: &mut MemAccess) {
        let l1_outcome = self.l1d.access(access);

        if let Some(addr) = l1_outcome.writeback_addr
            && let Some(further) = writeback_into(&mut self.l2, addr)
        {
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
        self.l1i.reset_stats();
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

/// Type of memory access
#[derive(PartialEq, Debug)]
pub enum AccessKind {
    Demand,
    Writeback,
    Prefetch,
}

/// Result of memory access
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
        if !num_sets.is_power_of_two() {
            return Err(AsterError::Config(
                "num_sets must be a power of two".to_string(),
            ));
        }

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
        let mut wb = MemAccess {
            addr,
            pc: 0,
            is_write: true,
            hit: None,
        };
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
        if already_present {
            return;
        }
        let mut pf_access = MemAccess {
            addr,
            pc: 0,
            is_write: false,
            hit: None,
        };
        self.do_access(&mut pf_access, AccessKind::Prefetch);
    }

    /// Simulates an access on the cache object given a memory address
    fn do_access(&mut self, access: &mut MemAccess, kind: AccessKind) -> AccessOutcome {
        if kind == AccessKind::Demand {
            self.accesses += 1;
        }
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
            if kind == AccessKind::Demand {
                self.hits += 1;
            } else {
                self.writebacks_received += 1;
            }
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
            return AccessOutcome {
                result: AccessResult::Hit,
                writeback_addr: None,
                prefetch_addrs,
            };
        }

        // if there is a miss, set an invalid line to the line or evict
        if kind == AccessKind::Demand {
            self.misses += 1;
        }
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
        AccessOutcome {
            result: AccessResult::Miss,
            writeback_addr,
            prefetch_addrs,
        }
    }

    fn determine_prefetch_addrs(
        &mut self,
        access: &mut MemAccess,
        hit_way: Option<usize>,
    ) -> Option<Vec<u64>> {
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
    use crate::config::{Config, default_prefetch_settings, default_repl_settings};
    use crate::trace_reader::MemAccess;

    fn dummy_access(addr: u64) -> MemAccess {
        MemAccess {
            addr,
            pc: 0,
            is_write: false,
            hit: None,
        }
    }

    fn write_access(addr: u64) -> MemAccess {
        MemAccess {
            addr,
            pc: 0,
            is_write: true,
            hit: None,
        }
    }

    fn cache_config(block_size: usize, cache_size: usize, associativity: usize) -> CacheConfig {
        CacheConfig {
            block_size,
            cache_size,
            associativity,
            replacement_policy: "lru".to_string(),
            prefetcher: None,
            repl_settings: default_repl_settings(),
            prefetch_settings: default_prefetch_settings(),
        }
    }

    fn cache_config_with_policy(
        block_size: usize,
        cache_size: usize,
        associativity: usize,
        policy: &str,
    ) -> CacheConfig {
        CacheConfig {
            replacement_policy: policy.to_string(),
            ..cache_config(block_size, cache_size, associativity)
        }
    }

    fn cache_config_with_prefetcher(
        block_size: usize,
        cache_size: usize,
        associativity: usize,
        prefetcher: &str,
    ) -> CacheConfig {
        CacheConfig {
            prefetcher: Some(prefetcher.to_string()),
            ..cache_config(block_size, cache_size, associativity)
        }
    }

    fn small_cache(associativity: usize, num_sets: usize) -> Cache {
        Cache::new(&cache_config(
            64,
            64 * associativity * num_sets,
            associativity,
        ))
        .unwrap()
    }

    #[test]
    fn lru_second_access_hits() {
        let mut cache = small_cache(4, 1);
        let mut a = dummy_access(0x1000);

        assert_eq!(cache.access(&mut a).result, AccessResult::Miss);
        assert_eq!(cache.access(&mut a).result, AccessResult::Hit);
    }

    // ---- Cache::new ----------------------------------------------------

    #[test]
    fn new_computes_num_sets_from_geometry() {
        let cache = Cache::new(&cache_config(64, 32768, 8)).unwrap();
        assert_eq!(cache.num_sets, 64); // 32768 / (64 * 8)
    }

    #[test]
    fn new_rejects_geometry_where_block_times_assoc_exceeds_cache_size() {
        // block_size * associativity (64*8=512) > cache_size (256) -> num_sets would be 0.
        match Cache::new(&cache_config(64, 256, 8)) {
            Err(_) => {}
            Ok(_) => panic!("expected an error, not a panic, for invalid geometry"),
        }
    }

    #[test]
    fn new_rejects_unknown_replacement_policy() {
        match Cache::new(&cache_config_with_policy(64, 32768, 8, "not_a_real_policy")) {
            Err(e) => assert_eq!(e.kind(), crate::error::ErrorKind::InvalidPolicyConfig),
            Ok(_) => panic!("expected an error for an unrecognized replacement policy"),
        }
    }

    #[test]
    fn new_rejects_unknown_prefetcher() {
        match Cache::new(&cache_config_with_prefetcher(
            64,
            32768,
            8,
            "not_a_real_prefetcher",
        )) {
            Err(e) => assert_eq!(e.kind(), crate::error::ErrorKind::Config),
            Ok(_) => panic!("expected an error for an unrecognized prefetcher"),
        }
    }

    // ---- Cache::access: eviction order & set isolation -----------------

    #[test]
    fn lru_evicts_the_correct_victim_once_associativity_is_full() {
        let mut cache = small_cache(2, 1);
        let (a, b, c) = (0x0000u64, 0x0040u64, 0x0080u64);

        assert_eq!(
            cache.access(&mut dummy_access(a)).result,
            AccessResult::Miss
        );
        assert_eq!(
            cache.access(&mut dummy_access(b)).result,
            AccessResult::Miss
        );
        // a and b now fill both ways; a is least recently used.
        assert_eq!(
            cache.access(&mut dummy_access(c)).result,
            AccessResult::Miss
        ); // evicts a

        // Check b before re-probing a: a is now absent, so probing it is
        // itself a miss that would trigger a further eviction (of b, being
        // the new LRU) -- probe order matters here.
        assert_eq!(
            cache.access(&mut dummy_access(b)).result,
            AccessResult::Hit,
            "b should still be resident"
        );
        assert_eq!(
            cache.access(&mut dummy_access(a)).result,
            AccessResult::Miss,
            "a should have been evicted"
        );
    }

    #[test]
    fn different_sets_do_not_interfere() {
        let mut cache = small_cache(1, 2); // 1-way, 2 sets
        let (a, b) = (0x0000u64, 0x0040u64); // block 0 -> set 0, block 1 -> set 1

        cache.access(&mut dummy_access(a));
        cache.access(&mut dummy_access(b));

        assert_eq!(cache.access(&mut dummy_access(a)).result, AccessResult::Hit);
        assert_eq!(cache.access(&mut dummy_access(b)).result, AccessResult::Hit);
    }

    // ---- dirty bit / writeback address on eviction ----------------------

    #[test]
    fn evicting_a_clean_line_produces_no_writeback() {
        let mut cache = small_cache(1, 1);
        cache.access(&mut dummy_access(0x0000)); // clean read
        let outcome = cache.access(&mut dummy_access(0x0040)); // evicts the clean line
        assert_eq!(outcome.writeback_addr, None);
    }

    #[test]
    fn evicting_a_dirty_line_returns_its_block_aligned_address() {
        let mut cache = small_cache(1, 1);
        cache.access(&mut write_access(0x0000)); // dirty write
        let outcome = cache.access(&mut write_access(0x0040)); // evicts the dirty line
        assert_eq!(outcome.writeback_addr, Some(0x0000));
    }

    // ---- Cache::writeback -------------------------------------------------

    #[test]
    fn writeback_to_an_absent_address_with_free_capacity_installs_without_evicting() {
        let mut cache = small_cache(2, 1);
        let outcome = cache.writeback(0x0000);
        assert_eq!(outcome.result, AccessResult::Miss);
        assert_eq!(outcome.writeback_addr, None);
    }

    #[test]
    fn writeback_to_a_resident_address_marks_it_dirty_without_evicting() {
        let mut cache = small_cache(1, 1);
        cache.access(&mut dummy_access(0x0000)); // clean read, installs the line
        let outcome = cache.writeback(0x0000);
        assert_eq!(outcome.result, AccessResult::Hit);
        assert_eq!(outcome.writeback_addr, None);

        // The address is now dirty purely from the writeback call: evicting
        // it should surface a writeback_addr where a plain read wouldn't have.
        let evict = cache.access(&mut dummy_access(0x0040));
        assert_eq!(evict.writeback_addr, Some(0x0000));
    }

    #[test]
    fn writeback_to_an_absent_address_that_evicts_a_dirty_line_reports_that_eviction() {
        let mut cache = small_cache(1, 1);
        cache.access(&mut write_access(0x0000)); // dirty
        let outcome = cache.writeback(0x0040); // absent, evicts 0x0000
        assert_eq!(outcome.result, AccessResult::Miss);
        assert_eq!(outcome.writeback_addr, Some(0x0000));
    }

    // ---- install_prefetch / prefetch bookkeeping -------------------------

    #[test]
    fn install_prefetch_on_a_fresh_address_marks_the_line_prefetched() {
        let mut cache = small_cache(1, 1);
        cache.install_prefetch(0x0000);
        assert!(cache.sets[0].lines[0].valid);
        assert!(cache.sets[0].lines[0].prefetched);
        assert_eq!(cache.sets[0].lines[0].tag, 0);
    }

    #[test]
    fn install_prefetch_on_an_already_resident_address_is_a_no_op() {
        let mut cache = small_cache(2, 1);
        cache.access(&mut dummy_access(0x0000));
        cache.access(&mut dummy_access(0x0040));

        cache.install_prefetch(0x0000); // already resident, must not disturb 0x0040

        assert_eq!(
            cache.access(&mut dummy_access(0x0040)).result,
            AccessResult::Hit
        );
    }

    #[test]
    fn demand_hit_on_a_prefetched_line_credits_prefetch_hits_and_clears_the_flag() {
        let mut cache = small_cache(1, 1);
        cache.install_prefetch(0x0000);
        assert_eq!(cache.prefetch_hits, 0);

        cache.access(&mut dummy_access(0x0000));

        assert_eq!(cache.prefetch_hits, 1);
        assert!(!cache.sets[0].lines[0].prefetched);
    }

    #[test]
    fn evicting_an_unused_prefetched_line_credits_prefetch_unused() {
        let mut cache = small_cache(1, 1);
        cache.install_prefetch(0x0000);
        assert_eq!(cache.prefetch_unused, 0);

        cache.access(&mut dummy_access(0x0040)); // evicts the never-demanded prefetch

        assert_eq!(cache.prefetch_unused, 1);
    }

    // ---- stats / reset_stats ----------------------------------------------

    #[test]
    fn stats_mirrors_hit_and_miss_getters() {
        let mut cache = small_cache(1, 1);
        cache.access(&mut dummy_access(0x0000)); // miss
        cache.access(&mut dummy_access(0x0000)); // hit

        let stats = cache.stats();
        assert_eq!(stats.hits, cache.get_hits() as u64);
        assert_eq!(stats.misses, cache.get_misses() as u64);
        assert_eq!((stats.hits, stats.misses), (1, 1));
    }

    #[test]
    fn reset_stats_zeroes_counters_but_preserves_cache_contents() {
        let mut cache = small_cache(1, 1);
        cache.access(&mut dummy_access(0x0000)); // miss, installs the line

        cache.reset_stats();
        assert_eq!(cache.get_hits(), 0);
        assert_eq!(cache.get_misses(), 0);
        assert_eq!(cache.accesses, 0);

        // the line itself must still be resident.
        assert_eq!(
            cache.access(&mut dummy_access(0x0000)).result,
            AccessResult::Hit
        );
        assert_eq!(cache.get_hits(), 1);
    }

    // ---- CacheHierarchy -----------------------------------------------------

    fn full_config(
        l1i: CacheConfig,
        l1d: CacheConfig,
        l2: CacheConfig,
        llc: CacheConfig,
    ) -> Config {
        Config { llc, l2, l1i, l1d }
    }

    fn uniform_config(block_size: usize, cache_size: usize, associativity: usize) -> Config {
        full_config(
            cache_config(block_size, cache_size, associativity),
            cache_config(block_size, cache_size, associativity),
            cache_config(block_size, cache_size, associativity),
            cache_config(block_size, cache_size, associativity),
        )
    }

    #[test]
    fn hierarchy_new_builds_all_four_levels() {
        let hierarchy = CacheHierarchy::new(uniform_config(64, 32768, 8)).unwrap();
        assert_eq!(hierarchy.l1i.num_sets, 64);
        assert_eq!(hierarchy.l1d.num_sets, 64);
        assert_eq!(hierarchy.l2.num_sets, 64);
        assert_eq!(hierarchy.llc.num_sets, 64);
    }

    #[test]
    fn hierarchy_new_propagates_an_invalid_l1i_config() {
        let config = full_config(
            cache_config(64, 256, 8), // invalid: block*assoc > cache_size
            cache_config(64, 32768, 8),
            cache_config(64, 32768, 8),
            cache_config(64, 32768, 8),
        );
        assert!(CacheHierarchy::new(config).is_err());
    }

    #[test]
    fn hierarchy_new_propagates_an_invalid_l2_config() {
        let config = full_config(
            cache_config(64, 32768, 8),
            cache_config(64, 32768, 8),
            cache_config(64, 256, 8), // invalid
            cache_config(64, 32768, 8),
        );
        assert!(CacheHierarchy::new(config).is_err());
    }

    #[test]
    fn access_instruction_never_touches_l1d() {
        let mut hierarchy = CacheHierarchy::new(uniform_config(64, 32768, 8)).unwrap();
        hierarchy.access_instruction(0x1000);
        hierarchy.access_instruction(0x1000);
        hierarchy.access_instruction(0x2000);

        assert_eq!(hierarchy.l1d.get_hits(), 0);
        assert_eq!(hierarchy.l1d.get_misses(), 0);
    }

    #[test]
    fn access_instruction_cascades_l1i_to_l2_to_llc_on_miss() {
        // l1i: 1 way / 1 set -> trivially evicted by any second address.
        // l2: 1 way / 4 sets -> retains a/b in different sets.
        let config = full_config(
            cache_config(64, 64, 1),
            cache_config(64, 32768, 8), // l1d, irrelevant here
            cache_config(64, 256, 1),   // l2: 4 sets, 1 way each
            cache_config(64, 32768, 8), // llc, generous
        );
        let mut hierarchy = CacheHierarchy::new(config).unwrap();
        let a = 0x0000u64; // l2 set 0
        let b = 0x0040u64; // l2 set 1 (block 1 & 3 == 1)

        hierarchy.access_instruction(a); // cold: l1i miss, l2 miss, llc miss
        assert_eq!(
            (
                hierarchy.l1i.get_misses(),
                hierarchy.l2.get_misses(),
                hierarchy.llc.get_misses()
            ),
            (1, 1, 1)
        );

        hierarchy.access_instruction(b); // l1i (1-way) evicts a; l2 (different set) misses fresh for b
        assert_eq!(
            (
                hierarchy.l1i.get_misses(),
                hierarchy.l2.get_misses(),
                hierarchy.llc.get_misses()
            ),
            (2, 2, 2)
        );

        hierarchy.access_instruction(a); // l1i misses again (b evicted a's slot), but l2 still holds a
        assert_eq!(hierarchy.l1i.get_misses(), 3);
        assert_eq!(
            hierarchy.l2.get_hits(),
            1,
            "a should still be resident in l2"
        );
        assert_eq!(
            hierarchy.llc.get_misses(),
            2,
            "llc must not be reached once l2 hits"
        );
    }

    #[test]
    fn access_data_read_hits_never_reach_l2_or_llc() {
        let mut hierarchy = CacheHierarchy::new(uniform_config(64, 32768, 8)).unwrap();
        hierarchy.access_data(&mut dummy_access(0x1000)); // cold miss: cascades to l2 and llc once
        hierarchy.access_data(&mut dummy_access(0x1000)); // repeated hit: l1d only
        hierarchy.access_data(&mut dummy_access(0x1000));

        assert_eq!(
            (hierarchy.l1d.get_hits(), hierarchy.l1d.get_misses()),
            (2, 1)
        );
        assert_eq!((hierarchy.l2.get_hits(), hierarchy.l2.get_misses()), (0, 1));
        assert_eq!(
            (hierarchy.llc.get_hits(), hierarchy.llc.get_misses()),
            (0, 1)
        );
    }

    #[test]
    fn access_data_cascades_a_dirty_l1d_eviction_through_a_full_l2_into_llc() {
        // l1d, l2: 1 way / 1 set each, so any second distinct address forces
        // an eviction. llc: 4 ways / 1 set, generous enough not to evict.
        let config = full_config(
            cache_config(64, 32768, 8), // l1i, unused here
            cache_config(64, 64, 1),    // l1d
            cache_config(64, 64, 1),    // l2
            cache_config(64, 256, 4),   // llc
        );
        let mut hierarchy = CacheHierarchy::new(config).unwrap();
        let (w, z, x) = (0x0000u64, 0x0040u64, 0x0080u64);

        // Pre-seed l1d with a dirty W and l2 with a dirty Z, bypassing
        // access_data so the next access_data call forces both a l1d
        // eviction *and* a full l2 to evict something of its own in the
        // same step -- exercising the nested writeback-cascade branch.
        hierarchy.l1d.access(&mut write_access(w));
        hierarchy.l2.access(&mut write_access(z));

        hierarchy.access_data(&mut write_access(x));

        // l1d now holds only x.
        assert!(hierarchy.l1d.sets[0].lines[0].valid);
        assert_eq!(hierarchy.l1d.sets[0].lines[0].tag, x / 64);
        assert!(hierarchy.l1d.sets[0].lines[0].dirty);

        // l2 now holds only x (w passed through and was itself evicted by x).
        assert_eq!(hierarchy.l2.sets[0].lines[0].tag, x / 64);
        assert!(hierarchy.l2.sets[0].lines[0].dirty);

        // llc must have absorbed both evicted dirty lines: w (via l1d's
        // writeback cascading through a full l2) and z (evicted from l2 to
        // make room for w), plus x itself once l2 missed for the demand access.
        let llc_tags: Vec<u64> = hierarchy.llc.sets[0]
            .lines
            .iter()
            .filter(|l| l.valid)
            .map(|l| l.tag)
            .collect();
        assert_eq!(llc_tags.len(), 3);
        assert!(llc_tags.contains(&(w / 64)));
        assert!(llc_tags.contains(&(z / 64)));
        assert!(llc_tags.contains(&(x / 64)));
        assert!(
            hierarchy.llc.sets[0]
                .lines
                .iter()
                .all(|l| !l.valid || l.dirty)
        );
    }

    #[test]
    fn reset_stats_should_also_reset_l1i() {
        let mut hierarchy = CacheHierarchy::new(uniform_config(64, 32768, 8)).unwrap();
        hierarchy.access_instruction(0x1000);
        hierarchy.access_instruction(0x1000);

        hierarchy.reset_stats();

        assert_eq!(
            hierarchy.l1i.get_hits(),
            0,
            "l1i hits should be cleared like every other level"
        );
        assert_eq!(
            hierarchy.l1i.get_misses(),
            0,
            "l1i misses should be cleared like every other level"
        );
    }
}
