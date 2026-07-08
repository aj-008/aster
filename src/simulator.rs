use crate::config::{Config, Args};
use crate::cache::{ CacheHierarchy };
use crate::error::AsterError;
use crate::trace_reader::{ TraceSource, open_trace };
use crate::stats::SimStats;

pub struct Simulator {
    trace_source: Box<dyn TraceSource>,
    warmup_inst: usize,
    simulation_inst: usize,
    hierarchy: CacheHierarchy,
}

impl Simulator { 
    pub fn new(config: Config, args: Args) -> Result<Self, AsterError> {
        let hierarchy = CacheHierarchy::new(config);

        let trace_source = open_trace(&args.trace)?;

        Ok(Self { trace_source, warmup_inst: args.warmup_instructions, simulation_inst: args.simulation_instructions, hierarchy })
    }

    pub fn run(&mut self) -> Result<SimStats, AsterError> {

        let mut instr_count: u64 = 0;
        loop {
            let instr = match self.trace_source.next_instruction() {
                Some(Ok(i)) => i,
                Some(Err(e)) => return Err(e),
                None => break,
            };

            instr_count += 1;

            for mut access in instr.mem_access() {
                self.hierarchy.access(&mut access);
            }

            if instr_count == self.warmup_inst as u64 {
                self.hierarchy.reset_stats();
            }
            if instr_count == self.simulation_inst as u64 + self.warmup_inst as u64 {
                break;
            }
        }

        Ok(SimStats::collect(&self.hierarchy, instr_count.saturating_sub(self.warmup_inst as u64)))
    }

}

