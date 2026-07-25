use crate::cache::CacheHierarchy;
use std::fmt;

/// Hit/miss counters for a single cache level.
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
}

impl CacheStats {
    /// Hit rate as a percentage (0.0 if there were no accesses).
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64 * 100.0
    }

    /// Misses per thousand instructions.
    pub fn mpki(&self, instructions: u64) -> f64 {
        if instructions == 0 {
            return 0.0;
        }
        self.misses as f64 / instructions as f64 * 1000.0
    }
}

/// Aggregated per-level stats plus the instruction count they cover,
/// produced at the end of a simulation run.
pub struct SimStats {
    pub l1i: CacheStats,
    pub l1d: CacheStats,
    pub l2: CacheStats,
    pub llc: CacheStats,
    pub instructions_simulated: u64,
}

impl SimStats {
    /// Snapshots stats from every level of `hierarchy` alongside the
    /// number of instructions the snapshot covers.
    pub fn collect(hierarchy: &CacheHierarchy, instructions: u64) -> Self {
        Self {
            l1i: hierarchy.l1i.stats(),
            l1d: hierarchy.l1d.stats(),
            l2: hierarchy.l2.stats(),
            llc: hierarchy.llc.stats(),
            instructions_simulated: instructions,
        }
    }
}

impl fmt::Display for SimStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let i = self.instructions_simulated;
        writeln!(f, "Instructions simulated: {}", i)?;
        writeln!(
            f,
            "{:<6} hits={:<10} misses={:<10} hit_rate={:.1}%  MPKI={:.2}",
            "L1I:",
            self.l1i.hits,
            self.l1i.misses,
            self.l1i.hit_rate(),
            self.l1i.mpki(i)
        )?;
        writeln!(
            f,
            "{:<6} hits={:<10} misses={:<10} hit_rate={:.1}%  MPKI={:.2}",
            "L1D:",
            self.l1d.hits,
            self.l1d.misses,
            self.l1d.hit_rate(),
            self.l1d.mpki(i)
        )?;
        writeln!(
            f,
            "{:<6} hits={:<10} misses={:<10} hit_rate={:.1}%  MPKI={:.2}",
            "L2:",
            self.l2.hits,
            self.l2.misses,
            self.l2.hit_rate(),
            self.l2.mpki(i)
        )?;
        writeln!(
            f,
            "{:<6} hits={:<10} misses={:<10} hit_rate={:.1}%  MPKI={:.2}",
            "LLC:",
            self.llc.hits,
            self.llc.misses,
            self.llc.hit_rate(),
            self.llc.mpki(i)
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CacheHierarchy;
    use crate::config::{CacheConfig, Config, default_prefetch_settings, default_repl_settings};

    fn cache_config() -> CacheConfig {
        CacheConfig {
            block_size: 64,
            cache_size: 32768,
            associativity: 8,
            replacement_policy: "lru".to_string(),
            prefetcher: None,
            repl_settings: default_repl_settings(),
            prefetch_settings: default_prefetch_settings(),
        }
    }

    fn hierarchy() -> CacheHierarchy {
        let config = Config { llc: cache_config(), l2: cache_config(), l1i: cache_config(), l1d: cache_config() };
        CacheHierarchy::new(config).unwrap()
    }

    // ---- CacheStats ------------------------------------------------------

    #[test]
    fn hit_rate_is_zero_with_no_accesses() {
        let stats = CacheStats { hits: 0, misses: 0 };
        assert_eq!(stats.hit_rate(), 0.0);
    }

    #[test]
    fn hit_rate_is_a_percentage() {
        let stats = CacheStats { hits: 3, misses: 1 };
        assert_eq!(stats.hit_rate(), 75.0);
    }

    #[test]
    fn hit_rate_all_misses_is_zero_percent() {
        let stats = CacheStats { hits: 0, misses: 4 };
        assert_eq!(stats.hit_rate(), 0.0);
    }

    #[test]
    fn hit_rate_all_hits_is_hundred_percent() {
        let stats = CacheStats { hits: 4, misses: 0 };
        assert_eq!(stats.hit_rate(), 100.0);
    }

    #[test]
    fn mpki_is_zero_with_no_instructions() {
        let stats = CacheStats { hits: 0, misses: 5 };
        assert_eq!(stats.mpki(0), 0.0);
    }

    #[test]
    fn mpki_is_misses_per_thousand_instructions() {
        let stats = CacheStats { hits: 0, misses: 5 };
        assert_eq!(stats.mpki(1000), 5.0);
        assert_eq!(stats.mpki(500), 10.0);
    }

    // ---- SimStats ----------------------------------------------------------

    #[test]
    fn collect_snapshots_every_level_and_the_instruction_count() {
        let mut h = hierarchy();
        h.access_instruction(0x1000);
        h.access_data(&mut crate::trace_reader::MemAccess { addr: 0x2000, pc: 0, is_write: false, hit: None });

        let stats = SimStats::collect(&h, 42);
        assert_eq!(stats.instructions_simulated, 42);
        assert_eq!(stats.l1i.misses, 1);
        assert_eq!(stats.l1d.misses, 1);
    }

    #[test]
    fn display_includes_instruction_count_and_every_printed_level() {
        let stats = SimStats {
            l1i: CacheStats { hits: 9, misses: 1 },
            l1d: CacheStats { hits: 8, misses: 2 },
            l2: CacheStats { hits: 1, misses: 1 },
            llc: CacheStats { hits: 1, misses: 0 },
            instructions_simulated: 1000,
        };
        let out = stats.to_string();
        assert!(out.contains("Instructions simulated: 1000"));
        assert!(out.contains("L1D:"));
        assert!(out.contains("L2:"));
        assert!(out.contains("LLC:"));
    }

    #[test]
    fn display_should_also_print_l1i() {
        let stats = SimStats {
            l1i: CacheStats { hits: 9, misses: 1 },
            l1d: CacheStats { hits: 8, misses: 2 },
            l2: CacheStats { hits: 1, misses: 1 },
            llc: CacheStats { hits: 1, misses: 0 },
            instructions_simulated: 1000,
        };
        assert!(stats.to_string().contains("L1I"), "L1I stats should appear in the printed report");
    }
}
