//! Aster: a cache replacement policy simulator.
//!
//! Reads a memory access trace, replays it through a configurable cache
//! hierarchy, and reports hit/miss statistics per level.

pub mod cache;
pub mod config;
pub mod error;
pub mod policies;
pub mod prefetch;
pub mod prefetchers;
pub mod policy;
pub mod simulator;
pub mod stats;
pub mod trace_reader;
