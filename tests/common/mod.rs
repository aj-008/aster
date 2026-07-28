//! Shared helpers for black-box integration tests. Only uses `aster`'s
//! public API plus already-declared dependencies (flate2) -- no new crates.

use flate2::Compression;
use flate2::write::GzEncoder;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn write_temp_bin_file(name_hint: &str, suffix: &str, contents: &[u8]) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "aster_integration_test_{name_hint}_{}_{}.{}",
        std::process::id(),
        n,
        suffix
    ));
    std::fs::File::create(&path)
        .unwrap()
        .write_all(contents)
        .unwrap();
    path
}

pub fn gzip_bytes(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// One raw 64-byte ChampSim instruction record: `ip`, up to 4 load
/// addresses, and up to 2 store addresses (0 == "no access", per ChampSim's
/// convention -- see `aster::trace_reader::InputInstruction::mem_access`).
pub fn raw_instruction_bytes(ip: u64, loads: [u64; 4], stores: [u64; 2]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(&ip.to_le_bytes());
    buf.push(0); // is_branch
    buf.push(0); // branch_taken
    buf.extend_from_slice(&[0u8; 2]); // dst_regs
    buf.extend_from_slice(&[0u8; 4]); // src_regs
    for s in stores {
        buf.extend_from_slice(&s.to_le_bytes());
    }
    for l in loads {
        buf.extend_from_slice(&l.to_le_bytes());
    }
    assert_eq!(buf.len(), 64);
    buf
}

/// Writes a gzip-compressed ChampSim trace file built from `(ip, load_addr)`
/// pairs (one load per instruction, no stores) and returns its path.
pub fn gzip_trace_path(name_hint: &str, instructions: &[(u64, u64)]) -> PathBuf {
    let mut raw = Vec::new();
    for &(ip, addr) in instructions {
        raw.extend(raw_instruction_bytes(ip, [addr, 0, 0, 0], [0, 0]));
    }
    write_temp_bin_file(name_hint, "champsimtrace.trace.gz", &gzip_bytes(&raw))
}
