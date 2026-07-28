//! Black-box tests for the public `open_trace` entry point, exercised only
//! through `aster`'s public API (unlike the white-box tests inline in
//! `src/trace_reader.rs`, which can see `ChampSimReader::new` and
//! `InputInstruction`'s private fields directly).

mod common;

use aster::error::ErrorKind;
use aster::trace_reader::open_trace;

#[test]
fn open_trace_rejects_unsupported_extension() {
    match open_trace("foo.bar") {
        Err(e) => assert_eq!(e.kind(), ErrorKind::InvalidTrace),
        Ok(_) => panic!("expected InvalidTrace for an unsupported extension"),
    }
}

#[test]
fn open_trace_rejects_missing_extension() {
    match open_trace("foo") {
        Err(e) => assert_eq!(e.kind(), ErrorKind::InvalidTrace),
        Ok(_) => panic!("expected InvalidTrace for a path with no extension"),
    }
}

#[test]
fn open_trace_rejects_a_nonexistent_file() {
    match open_trace("definitely_does_not_exist.champsimtrace.trace.gz") {
        Err(e) => assert_eq!(e.kind(), ErrorKind::Io),
        Ok(_) => panic!("expected an Io error for a missing file"),
    }
}

#[test]
fn open_trace_reads_a_gzip_champsim_trace_end_to_end() {
    let path = common::gzip_trace_path(
        "roundtrip",
        &[(0x1000, 0x2000), (0x1004, 0x2040), (0x1008, 0)],
    );

    let mut source = open_trace(path.to_str().unwrap()).unwrap();

    let first = source.next_instruction().unwrap().unwrap();
    assert_eq!(first.ip(), 0x1000);
    assert_eq!(first.mem_access().count(), 1);

    let second = source.next_instruction().unwrap().unwrap();
    assert_eq!(second.ip(), 0x1004);

    let third = source.next_instruction().unwrap().unwrap();
    assert_eq!(third.ip(), 0x1008);
    assert_eq!(
        third.mem_access().count(),
        0,
        "a zero load address means no access"
    );

    assert!(source.next_instruction().is_none());
    assert_eq!(source.instructions_read(), 3);
}
