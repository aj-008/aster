use crate::cache::CacheHierarchy;
use crate::config::{Args, Config};
use crate::error::AsterError;
use crate::stats::SimStats;
use crate::trace_reader::{TraceSource, open_trace};
use crate::reporter::{ConsoleReporter, Progress, Reporter, RunConfig};
use std::time::Instant;

/// Drives a trace through a [`CacheHierarchy`] for a warmup + measurement
/// window and produces final [`SimStats`].
pub struct Simulator {
    trace_source: Box<dyn TraceSource>,
    warmup_inst: usize,
    simulation_inst: usize,
    hierarchy: CacheHierarchy,
    reporter: Box<dyn Reporter>,
}

impl Simulator {
    /// Builds a `Simulator` from a parsed [`Config`] and [`Args`].
    ///
    /// # Errors
    /// Returns an [`AsterError`] if the trace at `args.trace` cannot be
    /// opened or has an unrecognized format.
    pub fn new(config: Config, args: Args) -> Result<Self, AsterError> {
        let run_config = RunConfig::from_args_and_config(&args, &config);
        let hierarchy = CacheHierarchy::new(config)?;
        let trace_source = open_trace(&args.trace)?;


        let mut reporter = Box::new(ConsoleReporter::new());
        reporter.on_start(&run_config);

        Ok(Self {
            trace_source,
            warmup_inst: args.warmup_instructions,
            simulation_inst: args.simulation_instructions,
            hierarchy,
            reporter,
        })
    }

    /// Replays the trace: `warmup_inst` instructions run to prime the
    /// hierarchy (stats then reset), followed by `simulation_inst`
    /// instructions of measurement. Stops early if the trace is shorter
    /// than `warmup_inst + simulation_inst`.
    ///
    /// # Errors
    /// Propagates any [`AsterError`] raised while reading the trace.
    pub fn run(&mut self) -> Result<SimStats, AsterError> {
        let mut instr_count: u64 = 0;
        let total_inst = (self.warmup_inst + self.simulation_inst) as u64;

        // HEARTBEAT AND PRINTING
        const HEARTBEAT_INTERVAL: u64 = 1_000_000;
        let mut next_heartbeat = HEARTBEAT_INTERVAL;
        let start = Instant::now();



        loop {
            let instr = match self.trace_source.next_instruction() {
                Some(Ok(i)) => i,
                Some(Err(e)) => return Err(e),
                None => break,
            };

            instr_count += 1;

            self.hierarchy.access_instruction(instr.ip());

            for mut access in instr.mem_access() {
                self.hierarchy.access_data(&mut access);
            }

            if instr_count == self.warmup_inst as u64 {
                self.hierarchy.reset_stats();
            }

            if instr_count >= next_heartbeat {
                let measured = instr_count.saturating_sub(self.warmup_inst as u64);
                let snapshot = SimStats::collect(&self.hierarchy, measured);
 
                self.reporter.on_heartbeat(&Progress {
                    insts_done: instr_count,
                    insts_total: total_inst,
                    elapsed: start.elapsed(),
                    live_hit_rates: vec![
                        ("L1I".to_string(), snapshot.l1i.hit_rate()),
                        ("L1D".to_string(), snapshot.l1d.hit_rate()),
                        ("L2".to_string(), snapshot.l2.hit_rate()),
                        ("LLC".to_string(), snapshot.llc.hit_rate()),
                    ],
                });
                next_heartbeat += HEARTBEAT_INTERVAL;
            }


            if instr_count == self.simulation_inst as u64 + self.warmup_inst as u64 {
                break;
            }
        }

        let final_stats = SimStats::collect(
            &self.hierarchy,
            instr_count.saturating_sub(self.warmup_inst as u64),
        );
        self.reporter.on_finish(&final_stats);
        Ok(final_stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CacheConfig, default_prefetch_settings, default_repl_settings};
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn write_temp_bin_file(name_hint: &str, suffix: &str, contents: &[u8]) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "aster_simulator_test_{name_hint}_{}_{}.{}",
            std::process::id(),
            n,
            suffix
        ));
        std::fs::File::create(&path).unwrap().write_all(contents).unwrap();
        path
    }

    fn gzip_bytes(data: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    /// One raw 64-byte ChampSim instruction record with a single load at
    /// `addr` (or no memory access at all if `addr` is 0).
    fn raw_instruction_bytes(ip: u64, addr: u64) -> Vec<u8> {
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(&ip.to_le_bytes());
        buf.push(0); // is_branch
        buf.push(0); // branch_taken
        buf.extend_from_slice(&[0u8; 2]); // dst_regs
        buf.extend_from_slice(&[0u8; 4]); // src_regs
        buf.extend_from_slice(&[0u8; 16]); // dst_mem[0..2]
        buf.extend_from_slice(&addr.to_le_bytes()); // src_mem[0]
        buf.extend_from_slice(&[0u8; 24]); // src_mem[1..4]
        assert_eq!(buf.len(), 64);
        buf
    }

    fn gzip_trace_path(name_hint: &str, instructions: &[(u64, u64)]) -> PathBuf {
        let mut raw = Vec::new();
        for &(ip, addr) in instructions {
            raw.extend(raw_instruction_bytes(ip, addr));
        }
        write_temp_bin_file(name_hint, "champsimtrace.trace.gz", &gzip_bytes(&raw))
    }

    fn cache_config() -> CacheConfig {
        CacheConfig {
            block_size: 64,
            cache_size: 4096,
            associativity: 4,
            replacement_policy: "lru".to_string(),
            prefetcher: None,
            repl_settings: default_repl_settings(),
            prefetch_settings: default_prefetch_settings(),
        }
    }

    fn config() -> Config {
        Config { llc: cache_config(), l2: cache_config(), l1i: cache_config(), l1d: cache_config() }
    }

    fn args(trace: PathBuf, warmup: usize, simulation: usize) -> Args {
        Args {
            config: PathBuf::from("default.toml"),
            trace: trace.to_str().unwrap().to_string(),
            simulation_instructions: simulation,
            warmup_instructions: warmup,
        }
    }

    /// `n` instructions, each touching a distinct, never-repeated data
    /// address (guaranteed cold miss) at a distinct ip.
    fn distinct_instructions(n: u64, addr_base: u64) -> Vec<(u64, u64)> {
        (0..n).map(|i| (0x1000 + i * 4, addr_base + i * 4096)).collect()
    }

    #[test]
    fn run_resets_stats_at_the_warmup_boundary() {
        let trace = gzip_trace_path("warmup_boundary", &distinct_instructions(5, 0x10000));
        let mut sim = Simulator::new(config(), args(trace, 2, 3)).unwrap();

        let stats = sim.run().unwrap();

        assert_eq!(stats.instructions_simulated, 3);
        assert_eq!(stats.l1d.misses, 3, "only the 3 post-warmup accesses should be counted");
    }

    #[test]
    fn run_stops_early_when_trace_is_shorter_than_warmup_plus_simulation() {
        let trace = gzip_trace_path("short_trace", &distinct_instructions(3, 0x20000));
        let mut sim = Simulator::new(config(), args(trace, 2, 5)).unwrap();

        let stats = sim.run().unwrap();

        // 3 instructions total, warmup consumes the first 2, leaving 1
        // measured instruction even though 5 were requested.
        assert_eq!(stats.instructions_simulated, 1);
        assert_eq!(stats.l1d.misses, 1);
    }

    #[test]
    fn run_when_warmup_meets_or_exceeds_trace_length_never_resets_stats() {
        // Documents current behavior: reset_stats() only fires when
        // instr_count exactly equals warmup_inst mid-loop. If the trace ends
        // first, warmup is never reached, so hits/misses from the *entire*
        // trace accumulate unreset, while instructions_simulated is
        // saturating-subtracted down to 0 -- a real (if narrow) UX quirk,
        // not asserted here as either correct or a bug to fix.
        let trace = gzip_trace_path("warmup_never_reached", &distinct_instructions(2, 0x30000));
        let mut sim = Simulator::new(config(), args(trace, 5, 10)).unwrap();

        let stats = sim.run().unwrap();

        assert_eq!(stats.instructions_simulated, 0);
        assert_eq!(stats.l1d.misses, 2, "both accesses ran before warmup was ever reached, and were never reset");
    }

    #[test]
    fn run_propagates_a_genuine_trace_read_error() {
        // A path with the gzip-dispatching extension but non-gzip content:
        // GzDecoder fails on the corrupt header with a non-UnexpectedEof
        // error, which must propagate out of run() rather than being
        // swallowed as a clean end of trace.
        let path = write_temp_bin_file("corrupt", "champsimtrace.trace.gz", b"not a gzip stream");
        let mut sim = Simulator::new(config(), args(path, 0, 10)).unwrap();

        assert!(sim.run().is_err());
    }

    #[test]
    fn new_propagates_hierarchy_construction_errors_before_touching_the_trace() {
        let mut bad_config = config();
        bad_config.l1i.cache_size = 32; // block_size(64) * associativity(4) > cache_size(32)

        // The trace path does not need to exist: hierarchy construction is
        // validated first and should fail before the trace is ever opened.
        let result = Simulator::new(bad_config, args(PathBuf::from("does_not_exist.champsimtrace.trace.gz"), 0, 10));
        assert!(result.is_err());
    }

    #[test]
    fn new_propagates_trace_open_errors() {
        let result = Simulator::new(config(), args(PathBuf::from("no_such_extension.foo"), 0, 10));
        assert!(result.is_err());
    }
}
