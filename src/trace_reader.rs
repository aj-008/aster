use crate::error::AsterError;
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

/// A single load or store extracted from an [`InputInstruction`], passed
/// through the cache hierarchy. `hit` is filled in by [`crate::cache::Cache::access`].
pub struct MemAccess {
    pub addr: u64,
    pub pc: u64,
    pub is_write: bool,
    pub hit: Option<bool>,
}

/// Source of instructions to replay through the simulator. Implementations
/// decode a trace format (e.g. ChampSim's binary format) into
/// [`InputInstruction`]s.
pub trait TraceSource {
    /// Returns the next instruction, `None` at end of trace, or `Some(Err)`
    /// on a read/parse failure.
    fn next_instruction(&mut self) -> Option<Result<InputInstruction, AsterError>>;
    /// Number of instructions successfully read so far.
    fn instructions_read(&self) -> usize;
}

/// Opens a trace file at `path`, selecting a [`TraceSource`] implementation
/// based on its extension (`.gz` is stripped before matching).
///
/// # Errors
/// Returns [`AsterError::InvalidTrace`] if the extension is unrecognized or
/// missing, or [`AsterError::Io`] if the file cannot be opened.
pub fn open_trace(path: &str) -> Result<Box<dyn TraceSource>, AsterError> {
    let stem = path.strip_suffix(".trace.gz").unwrap_or(path);

    match Path::new(stem).extension().and_then(|e| e.to_str()) {
        Some("champsimtrace") => Ok(Box::new(ChampSimReader::from_path(path)?)),
        Some(ext) => Err(AsterError::InvalidTrace {
            fmt: ext.to_string(),
        }),
        None => Err(AsterError::InvalidTrace {
            fmt: "no extension".to_string(),
        }),
    }
}

// ChampSim only below
const NUM_INSTR_DESTINATIONS: usize = 2;
const NUM_INSTR_SOURCES: usize = 4;

// Currently the ChampSim instruction format. need to add a
// wrapper called mem access to act  as a separater in the
// trace reader level to pass to the cache hierarchy
#[repr(C)]
#[derive(Debug)]
pub struct InputInstruction {
    ip: u64,
    is_branch: u8,
    branch_taken: u8,
    dst_regs: [u8; NUM_INSTR_DESTINATIONS],
    src_regs: [u8; NUM_INSTR_SOURCES],
    dst_mem: [u64; NUM_INSTR_DESTINATIONS],
    src_mem: [u64; NUM_INSTR_SOURCES],
}

impl InputInstruction {
    /// Yields one [`MemAccess`] per nonzero load/store address, loads
    /// before stores. A zero address is treated as "no access" (ChampSim's
    /// convention for unused source/dest slots).
    pub fn mem_access(&self) -> impl Iterator<Item = MemAccess> + '_ {
        let loads = self
            .src_mem
            .iter()
            .filter(|&&a| a != 0)
            .map(|&addr| MemAccess {
                addr,
                pc: self.ip,
                is_write: false,
                hit: None,
            });
        let stores = self
            .dst_mem
            .iter()
            .filter(|&&a| a != 0)
            .map(|&addr| MemAccess {
                addr,
                pc: self.ip,
                is_write: true,
                hit: None,
            });

        loads.chain(stores)
    }

    pub fn ip(&self) -> u64 {
        self.ip
    }
}

/// [`TraceSource`] for ChampSim's fixed-size binary instruction trace
/// format, optionally gzip-compressed.
pub struct ChampSimReader<R: Read> {
    reader: BufReader<R>,
    pub instructions_read: usize,
}


// There is an opiton to use bytemuck here to remove the unsafe block 
// here with macros to ensure no padding on InputInstruction struct
impl<R: Read> ChampSimReader<R> {
    /// Reads and decodes one fixed-size [`InputInstruction`] record.
    ///
    /// # Errors
    /// Returns [`AsterError::Io`] (with `io::ErrorKind::UnexpectedEof` at a
    /// clean end of trace, or another `io::Error` kind on a genuine I/O
    /// failure) if the record cannot be read in full.
    pub fn read_instruction(&mut self) -> Result<InputInstruction, AsterError> {
        let mut buf = [0u8; size_of::<InputInstruction>()];
        self.reader.read_exact(&mut buf)?;
        Ok(unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const InputInstruction) })
    }

    fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
            instructions_read: 0,
        }
    }
}

impl ChampSimReader<GzDecoder<File>> {
    /// Opens a gzip-compressed ChampSim trace file at `path`.
    ///
    /// # Errors
    /// Returns an `io::Error` if the file cannot be opened.
    pub fn from_path(path: &str) -> io::Result<Self> {
        let file = File::open(path)?;
        let decoder = GzDecoder::new(file);
        Ok(Self::new(decoder))
    }
}

impl<R: Read> TraceSource for ChampSimReader<R> {
    fn next_instruction(&mut self) -> Option<Result<InputInstruction, AsterError>> {
        match self.read_instruction() {
            Ok(instr) => {
                self.instructions_read += 1;
                Some(Ok(instr))
            }
            Err(AsterError::Io(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => None,
            Err(e) => Some(Err(e)),
        }
    }

    fn instructions_read(&self) -> usize {
        self.instructions_read
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::{Cursor, Write};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn write_temp_bin_file(name_hint: &str, suffix: &str, contents: &[u8]) -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "aster_trace_test_{name_hint}_{}_{}.{}",
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

    /// Builds one raw 64-byte ChampSim instruction record, matching
    /// `InputInstruction`'s field layout exactly (see `parse_known_instruction`
    /// for the byte-for-byte breakdown this mirrors).
    fn raw_instruction_bytes(ip: u64, loads: [u64; 4], stores: [u64; 2]) -> Vec<u8> {
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

    fn blank_instruction(ip: u64) -> InputInstruction {
        InputInstruction {
            ip,
            is_branch: 0,
            branch_taken: 0,
            dst_regs: [0, 0],
            src_regs: [0, 0, 0, 0],
            dst_mem: [0, 0],
            src_mem: [0, 0, 0, 0],
        }
    }

    #[test]
    fn parse_known_instruction() {
        // bytes of first instruction from 462.libquantum trace
        let raw: Vec<u8> = vec![
            0x42, 0x14, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, // ip
            0x00, // is_branch
            0x00, // branch_taken
            0x03, 0x00, // dst_regs
            0x0d, 0x00, 0x00, 0x00, // src_regs
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // dst_mem[0]
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // dst_mem[1]
            0xc8, 0x05, 0xf1, 0xee, 0x43, 0x7f, 0x00, 0x00, // src_mem[0]
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // src_mem[1]
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // src_mem[2]
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // src_mem[3]
        ];
        assert_eq!(raw.len(), 64);

        let cursor = Cursor::new(raw);
        let mut reader = ChampSimReader::new(cursor);
        let instr = reader.read_instruction().unwrap();

        assert_eq!(instr.ip, 0x0000000000401442);
        assert_eq!(instr.is_branch, 0);
        assert_eq!(instr.branch_taken, 0);
        assert_eq!(instr.dst_regs[0], 3);
        assert_eq!(instr.src_regs[0], 13);
        assert_eq!(instr.src_mem[0], 0x00007f43eef105c8);
        assert_eq!(instr.dst_mem[0], 0);
    }

    #[test]
    fn mem_access_yields_loads_before_stores_and_skips_zero_addresses() {
        let mut instr = blank_instruction(0xdead);
        instr.src_mem = [0x1000, 0, 0x1008, 0]; // two real loads, two unused slots
        instr.dst_mem = [0x2000, 0]; // one real store, one unused slot

        let accesses: Vec<MemAccess> = instr.mem_access().collect();
        assert_eq!(accesses.len(), 3);

        assert_eq!(accesses[0].addr, 0x1000);
        assert!(!accesses[0].is_write);
        assert_eq!(accesses[1].addr, 0x1008);
        assert!(!accesses[1].is_write);
        assert_eq!(accesses[2].addr, 0x2000);
        assert!(accesses[2].is_write);

        assert!(accesses.iter().all(|a| a.pc == 0xdead));
        assert!(accesses.iter().all(|a| a.hit.is_none()));
    }

    #[test]
    fn mem_access_yields_nothing_when_all_addresses_are_zero() {
        let instr = blank_instruction(0x10);
        assert_eq!(instr.mem_access().count(), 0);
    }

    #[test]
    fn ip_getter_returns_instruction_pointer() {
        assert_eq!(blank_instruction(0x4242).ip(), 0x4242);
    }

    #[test]
    fn open_trace_rejects_unsupported_extension() {
        match open_trace("foo.bar") {
            Err(e) => assert_eq!(e.kind(), crate::error::ErrorKind::InvalidTrace),
            Ok(_) => panic!("expected InvalidTrace for an unsupported extension"),
        }
    }

    #[test]
    fn open_trace_rejects_missing_extension() {
        match open_trace("foo") {
            Err(e) => assert_eq!(e.kind(), crate::error::ErrorKind::InvalidTrace),
            Ok(_) => panic!("expected InvalidTrace for a path with no extension"),
        }
    }

    #[test]
    fn open_trace_reads_gzip_champsim_trace_end_to_end() {
        let mut raw = raw_instruction_bytes(0x1000, [0x2000, 0, 0, 0], [0, 0]);
        raw.extend(raw_instruction_bytes(0x1004, [0, 0, 0, 0], [0x3000, 0]));
        let compressed = gzip_bytes(&raw);
        let path = write_temp_bin_file("roundtrip", "champsimtrace.trace.gz", &compressed);

        let mut source = open_trace(path.to_str().unwrap()).unwrap();
        let first = source.next_instruction().unwrap().unwrap();
        assert_eq!(first.ip(), 0x1000);
        let second = source.next_instruction().unwrap().unwrap();
        assert_eq!(second.ip(), 0x1004);
        assert!(source.next_instruction().is_none());
        assert_eq!(source.instructions_read(), 2);
    }

    #[test]
    fn open_trace_on_champsimtrace_extension_always_assumes_gzip() {
        // A path ending in ".champsimtrace" with no ".trace.gz" suffix still
        // dispatches to the gzip-only ChampSimReader -- there is no code
        // path for reading an uncompressed trace. Genuine (uncompressed)
        // ChampSim binary bytes written to such a path are therefore
        // misread as a corrupt gzip stream, even though the bytes
        // themselves are perfectly valid instruction records. This is
        // current, intentional-looking dispatch behavior (documented here,
        // not treated as a bug to fix).
        let raw = raw_instruction_bytes(0x1000, [0x2000, 0, 0, 0], [0, 0]);
        let path = write_temp_bin_file("uncompressed", "champsimtrace", &raw);

        let mut source = open_trace(path.to_str().unwrap()).unwrap();
        match source.next_instruction() {
            Some(Err(_)) => {}
            other => panic!(
                "expected an uncompressed .champsimtrace file to surface a read error, got {other:?}"
            ),
        }
    }

    #[test]
    fn next_instruction_returns_none_at_clean_eof() {
        let mut reader = ChampSimReader::new(Cursor::new(Vec::<u8>::new()));
        assert!(reader.next_instruction().is_none());
        assert_eq!(reader.instructions_read(), 0);
    }

    #[test]
    fn next_instruction_silently_drops_a_truncated_trailing_record() {
        // A trace file cut off mid-record is, at the io::ErrorKind level,
        // indistinguishable from a clean end of trace: read_exact reports
        // UnexpectedEof either way. The complete leading instruction is
        // still returned; the trailing partial record is silently dropped
        // with no error surfaced. Documents current behavior -- not
        // asserted to be either "fixed" or a regression here.
        let mut raw = raw_instruction_bytes(0x1000, [0x2000, 0, 0, 0], [0, 0]);
        raw.extend_from_slice(&[0xAAu8; 30]); // partial second record
        let mut reader = ChampSimReader::new(Cursor::new(raw));

        let first = reader.next_instruction().unwrap().unwrap();
        assert_eq!(first.ip(), 0x1000);
        assert!(reader.next_instruction().is_none());
        assert_eq!(reader.instructions_read(), 1);
    }

    /// A `Read` that serves `good_bytes` and then fails with a non-EOF error,
    /// simulating e.g. a disk failure or a corrupt gzip stream mid-trace.
    struct FlakyReader {
        good_bytes: Vec<u8>,
        position: usize,
    }

    impl Read for FlakyReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.position >= self.good_bytes.len() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "simulated disk failure",
                ));
            }
            let n = std::cmp::min(buf.len(), self.good_bytes.len() - self.position);
            buf[..n].copy_from_slice(&self.good_bytes[self.position..self.position + n]);
            self.position += n;
            Ok(n)
        }
    }

    #[test]
    fn next_instruction_propagates_genuine_io_errors_instead_of_swallowing_them() {
        let good = raw_instruction_bytes(0x1000, [0, 0, 0, 0], [0, 0]);
        let mut reader = ChampSimReader::new(FlakyReader { good_bytes: good, position: 0 });

        let first = reader.next_instruction().unwrap().unwrap();
        assert_eq!(first.ip(), 0x1000);

        match reader.next_instruction() {
            Some(Err(e)) => assert_eq!(e.kind(), crate::error::ErrorKind::Io),
            other => panic!("expected a propagated I/O error, got {other:?}"),
        }
    }

    #[test]
    fn instructions_read_counts_only_successful_reads() {
        let good = raw_instruction_bytes(0x1000, [0, 0, 0, 0], [0, 0]);
        let mut reader = ChampSimReader::new(Cursor::new(good));
        assert_eq!(reader.instructions_read(), 0);
        assert!(reader.next_instruction().unwrap().is_ok());
        assert_eq!(reader.instructions_read(), 1);
        assert!(reader.next_instruction().is_none());
        assert_eq!(reader.instructions_read(), 1, "a clean EOF must not bump the counter");
    }
}
