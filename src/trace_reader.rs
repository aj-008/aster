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
    use std::io::Cursor;

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
}
