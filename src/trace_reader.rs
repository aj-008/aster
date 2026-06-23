use std::io::{self, BufReader, Read};
use std::fs::File;
use flate2::read::GzDecoder;


pub struct TraceReader<R: Read> {
    reader: BufReader<R>,
}

const NUM_INSTR_DESTINATIONS: usize = 2;
const NUM_INSTR_SOURCES: usize  = 4;


#[repr(C)]
#[derive(Debug)]
pub struct InputInstruction {
    pub ip: u64,
    pub is_branch: u8,    
    pub branch_taken: u8,
    pub dst_regs: [u8; NUM_INSTR_DESTINATIONS],
    pub src_regs: [u8; NUM_INSTR_SOURCES],
    pub dst_mem: [u64; NUM_INSTR_DESTINATIONS],
    pub src_mem: [u64; NUM_INSTR_SOURCES],
}




impl<R: Read> TraceReader<R>{
    pub fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
        }
    }

    pub fn read_instruction(&mut self) -> Result<InputInstruction, io::Error> {
        let mut buf = [0u8; size_of::<InputInstruction>()];
        self.reader.read_exact(&mut buf)?;
        Ok(unsafe { std::ptr::read(buf.as_ptr() as *const InputInstruction) })
    }
}


impl TraceReader<GzDecoder<File>> {
    pub fn from_path(path: &str) -> io::Result<Self> {
        let file = File::open(path)?;
        let decoder = GzDecoder::new(file);
        Ok(Self::new(decoder))
    }
}


impl<R: Read> Iterator for TraceReader<R> {
    type Item = InputInstruction;

    fn next(&mut self) -> Option<Self::Item> {
        match self.read_instruction() {
            Ok(val) => Some(val),
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => None,
            Err(err) => panic!("Fatal: Failed to read instruction. {}", err),
        }
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
            0x42, 0x14, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00,  // ip
            0x00,                                            // is_branch
            0x00,                                            // branch_taken
            0x03, 0x00,                                      // dst_regs
            0x0d, 0x00, 0x00, 0x00,                          // src_regs
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // dst_mem[0]
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // dst_mem[1]
            0xc8, 0x05, 0xf1, 0xee, 0x43, 0x7f, 0x00, 0x00,  // src_mem[0]
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // src_mem[1]
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // src_mem[2]
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // src_mem[3]
        ];
        assert_eq!(raw.len(), 64); 

        let cursor = Cursor::new(raw);
        let mut reader = TraceReader::new(cursor);
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
