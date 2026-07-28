//! End-to-end black-box tests driving `Simulator::run` over synthetic
//! gzip-compressed ChampSim traces, using only `aster`'s public API -- this
//! is the same contract a real library consumer or the `aster` binary
//! itself would go through.

mod common;

use aster::config::{Args, CacheConfig, Config, default_prefetch_settings, default_repl_settings};
use aster::simulator::Simulator;
use std::path::PathBuf;

fn cache_config(policy: &str) -> CacheConfig {
    CacheConfig {
        block_size: 64,
        cache_size: 4096,
        associativity: 4,
        replacement_policy: policy.to_string(),
        prefetcher: None,
        repl_settings: default_repl_settings(),
        prefetch_settings: default_prefetch_settings(),
    }
}

fn config_with_l2_policy(policy: &str) -> Config {
    Config {
        llc: cache_config("lru"),
        l2: cache_config(policy),
        l1i: cache_config("lru"),
        l1d: cache_config("lru"),
    }
}

fn args(trace: PathBuf, warmup: usize, simulation: usize) -> Args {
    Args {
        config: PathBuf::from("default.toml"),
        trace: trace.to_str().unwrap().to_string(),
        simulation_instructions: simulation,
        warmup_instructions: warmup,
    }
}

#[test]
fn full_pipeline_runs_over_a_repeating_working_set_and_warms_up() {
    // A small working set (4 addresses) accessed in a loop, repeated many
    // times, followed by a fresh cold address on the very last instruction.
    // After warmup the working set should be resident, so the measurement
    // window should see a healthy L1D hit rate.
    let working_set = [0x10000u64, 0x10040, 0x10080, 0x100C0];
    let mut instructions = Vec::new();
    for (i, &addr) in std::iter::repeat_n(&working_set, 20).flatten().enumerate() {
        instructions.push((0x1000 + i as u64 * 4, addr));
    }
    let trace = common::gzip_trace_path("working_set", &instructions);

    let total = instructions.len();
    let warmup = 8; // a couple of full passes through the 4-address loop
    let simulation = total - warmup;
    let mut sim = Simulator::new(
        config_with_l2_policy("lru"),
        args(trace, warmup, simulation),
    )
    .unwrap();

    let stats = sim.run().unwrap();

    assert_eq!(stats.instructions_simulated as usize, simulation);
    assert!(
        stats.l1d.hit_rate() > 50.0,
        "a small repeating working set should mostly hit in L1D once warm, got {}",
        stats.l1d.hit_rate()
    );
}

#[test]
fn full_pipeline_works_with_srrip_configured_on_l2() {
    // Regression coverage for the SRRIP settings-validation fix: this must
    // not error out on default SRRIP settings, unlike before that fix.
    let instructions: Vec<(u64, u64)> = (0..10)
        .map(|i| (0x2000 + i * 4, 0x40000 + (i % 3) * 4096))
        .collect();
    let trace = common::gzip_trace_path("srrip_pipeline", &instructions);

    let mut sim = Simulator::new(config_with_l2_policy("srrip"), args(trace, 2, 8)).unwrap();
    let stats = sim.run().unwrap();

    assert_eq!(stats.instructions_simulated, 8);
}

#[test]
fn cold_trace_has_a_zero_percent_l1d_hit_rate() {
    // Every address touched exactly once: every access is a cold miss.
    let instructions: Vec<(u64, u64)> = (0..6)
        .map(|i| (0x3000 + i * 4, 0x50000 + i * 4096))
        .collect();
    let trace = common::gzip_trace_path("all_cold", &instructions);

    let mut sim = Simulator::new(config_with_l2_policy("lru"), args(trace, 0, 6)).unwrap();
    let stats = sim.run().unwrap();

    assert_eq!(stats.l1d.hits, 0);
    assert_eq!(stats.l1d.misses, 6);
    assert_eq!(stats.l1d.hit_rate(), 0.0);
}

#[test]
fn config_validation_errors_surface_before_any_trace_is_touched() {
    let mut bad = config_with_l2_policy("lru");
    bad.l1d.cache_size = 32; // block_size(64) * associativity(4) > cache_size(32)

    let result = Simulator::new(
        bad,
        args(PathBuf::from("irrelevant.champsimtrace.trace.gz"), 0, 1),
    );
    assert!(result.is_err());
}
