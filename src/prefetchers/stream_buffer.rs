use crate::{error::AsterError, prefetch::Prefetcher};

use serde::Deserialize;

#[derive(Deserialize)]
struct StreamBufferSettings {
    #[serde(default = "default_degree")]
    degree: usize,
    #[serde(default = "default_num_streams")]
    num_streams: usize,
}

impl TryFrom<&toml::Value> for StreamBufferSettings {
    type Error = AsterError;
    fn try_from(value: &toml::Value) -> Result<Self, Self::Error> {
        value
            .clone()
            .try_into()
            .map_err(|e| AsterError::InvalidPolicyConfig(e.to_string()))
    }
}

struct StreamEntry {
    last_addr: u64,
    stride: i64,
    confirmed: bool,
}
pub struct StreamBuffer {
    streams: Vec<StreamEntry>,
    degree: usize,
    block_size: usize,
    next_evict: usize,
}

fn default_degree() -> usize {
    3
}

fn default_num_streams() -> usize {
    8
}

impl StreamBuffer {
    pub fn new(block_size: usize, settings: &toml::Value) -> Result<Self, AsterError> {
        let s: StreamBufferSettings = settings.try_into()?;

        let streams = (0..s.num_streams)
            .map(|_| StreamEntry { last_addr: 0, stride: 0, confirmed: false })
            .collect();

        Ok(Self {
            streams,
            degree: s.degree,
            block_size,
            next_evict: 0,
        })
    }
}

impl Prefetcher for StreamBuffer {
    fn observe(&mut self, addr: u64, _pc: u64, _hit: bool) -> Vec<u64> {
        let block = (addr / self.block_size as u64) as i64;
        
        for entry in &mut self.streams {
            let candidate_stride = block - (entry.last_addr / self.block_size as u64) as i64;
            if candidate_stride == entry.stride {
                let was_confirmed = entry.confirmed;
                entry.confirmed = true;
                entry.last_addr = addr;
                if was_confirmed {
                    return (1..=self.degree as i64)
                        .map(|i| ((block + entry.stride * i) as u64) * self.block_size as u64)
                        .collect();
                }
            }
        }
        
        let slot = self.next_evict;
        self.next_evict = (self.next_evict + 1) % self.streams.len();
        self.streams[slot] = StreamEntry { last_addr: addr, stride: 1, confirmed: false };
        Vec::new()
    }
}
