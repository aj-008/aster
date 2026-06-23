use crate::config::{Config, Args};
use crate::cache::{self, AccessResult, Cache};
use crate::trace_reader::TraceReader;
use std::io::Read;

pub struct Simulator {
    instr_count: usize,
    warmup_inst: usize,
    simulation_inst: usize,
    l1d: cache::Cache,
    l2: cache::Cache,
    llc: cache::Cache,
}

impl Simulator { 
    pub fn new(config: Config, args: Args) -> Self {
        let l1d = Cache::new(
            config.l1d.block_size, 
            config.l1d.associativity,
            config.l1d.cache_size,
            config.l1d.replacement_policy
            .as_deref().unwrap_or("lru"),
        );

        let l2 = Cache::new(
            config.l2.block_size, 
            config.l2.associativity,
            config.l2.cache_size,
            config.l2.replacement_policy
            .as_deref().unwrap_or("lru"),
        );

        let llc = Cache::new(
            config.llc.block_size, 
            config.llc.associativity,
            config.llc.cache_size,
            config.llc.replacement_policy
            .as_deref().unwrap_or("lru"),
        );

        Self { instr_count: 0, warmup_inst: 0, simulation_inst: args.simulation_instructions, l1d, l2, llc }
    }

    pub fn run<R: Read>(mut self, trace_reader: TraceReader<R>) {
        // instantiate cache object here? is there only one or one for each level?
        // let cache = Cache::new(self.config.sets, self.config.ways, ... etc);
        
        // This can be turned into a for loop if the reader object is given the iterator trait
        for instr in trace_reader {
            self.instr_count += 1;

            let all_mem = instr.src_mem.iter().chain(instr.dst_mem.iter());
            for addr in all_mem.filter(|&&a| a != 0) {
                if self.l1d.access(*addr) == AccessResult::Miss 
                    && self.l2.access(*addr) == AccessResult::Miss {
                        self.llc.access(*addr);
                }
            }
                if self.instr_count > self.simulation_inst {
                    break;
                }
        }
        self.report();
    }

    fn report(&self) {
        println!("instructions: {}", self.instr_count);
        println!("LLC hits: {}, LLC misses: {}", self.llc.get_hits(), self.llc.get_misses());
        println!("L1 hits: {}, L1 misses: {}", self.l1d.get_hits(), self.l1d.get_misses());
        println!("L2 hits: {}, L2 misses: {}", self.l2.get_hits(), self.l2.get_misses());
    }
}

