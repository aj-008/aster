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
