use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct RunConfig {
    pub trace_path: String,
    pub warmup_insts: u64,
    pub sim_insts: u64,
    pub caches: Vec<CacheConfigSummary>,
}
 

#[derive(Debug, Clone)]
pub struct CacheConfigSummary {
    pub name: String,
    pub block_size: u32,
    pub cache_size: u32,
    pub associativity: u32,
    pub replacement_policy: String,
    pub prefetcher: Option<String>,
    pub repl_settings_debug: String,
    pub prefetch_settings_debug: String,

}
 

#[derive(Debug, Clone)]
pub struct Progress {
    pub insts_done: u64,
    pub insts_total: u64,
    pub elapsed: Duration,
    pub live_hit_rates: Vec<(String, f64)>,
}
 

fn format_toml_compact(v: &toml::Value) -> String {
    match v {
        toml::Value::Table(map) => map
            .iter()
            .map(|(k, v)| format!("{}={}", k, format_toml_scalar(v)))
            .collect::<Vec<_>>()
            .join(", "),
        other => format_toml_scalar(other),
    }
}


fn format_toml_scalar(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Datetime(d) => d.to_string(),
        toml::Value::Array(a) => format!(
            "[{}]",
            a.iter().map(format_toml_scalar).collect::<Vec<_>>().join(", ")
        ),
        toml::Value::Table(_) => format_toml_compact(v),
    }
}

impl From<(&str, &crate::config::CacheConfig)> for CacheConfigSummary {
    fn from((name, c): (&str, &crate::config::CacheConfig)) -> Self {
        CacheConfigSummary {
            name: name.to_string(),
            block_size: c.block_size as u32,
            cache_size: c.cache_size as u32,
            associativity: c.associativity as u32,
            replacement_policy: c.replacement_policy.clone(),
            prefetcher: c.prefetcher.clone(),
            repl_settings_debug: format_toml_compact(&c.repl_settings),
            prefetch_settings_debug: format_toml_compact(&c.prefetch_settings),

        }
    }
}
 
impl RunConfig {
    pub fn from_args_and_config(args: &crate::config::Args, config: &crate::config::Config) -> Self {
        RunConfig {
            trace_path: args.trace.clone(),
            warmup_insts: args.warmup_instructions as u64,
            sim_insts: args.simulation_instructions as u64,
            caches: vec![
                ("L1I", &config.l1i).into(),
                ("L1D", &config.l1d).into(),
                ("L2", &config.l2).into(),
                ("LLC", &config.llc).into(),
            ],
        }
    }
}
 

pub trait Reporter {
    fn on_start(&mut self, config: &RunConfig);
    fn on_heartbeat(&mut self, progress: &Progress);
    fn on_finish(&mut self, results: &crate::stats::SimStats);
}
 

pub struct ConsoleReporter {
    start_time: Instant,
}
 
impl ConsoleReporter {
    pub fn new() -> Self {
        Self { start_time: Instant::now() }
    }
}
 
impl Reporter for ConsoleReporter {
    fn on_start(&mut self, config: &RunConfig) {
        self.start_time = Instant::now();
        println!("=== Aster Simulator ===");
        println!("trace:        {}", config.trace_path);
        println!("warmup insts: {}", config.warmup_insts);
        println!("sim insts:    {}", config.sim_insts);
        println!("--- cache config ---");
        for c in &config.caches {
            println!(
                "  {:<5} block_size={:<5} cache_size={:<8} assoc={:<3} policy={:<10} prefetcher={}",
                c.name,
                c.block_size,
                c.cache_size,
                c.associativity,
                c.replacement_policy,
                c.prefetcher.as_deref().unwrap_or("none")
            );
            if !c.repl_settings_debug.is_empty() {
                println!("        repl_settings: {}", c.repl_settings_debug);
            }
            if !c.prefetch_settings_debug.is_empty() {
                println!("        prefetch_settings: {}", c.prefetch_settings_debug);
            }

            println!("----------------------------");

        }
    }
 
    fn on_heartbeat(&mut self, p: &Progress) {
        let pct = 100.0 * p.insts_done as f64 / p.insts_total as f64;
        print!(
            "\rheartbeat: {:>10}/{:<10} insts ({:>5.1}%)  elapsed={:>6.1}s",
            p.insts_done,
            p.insts_total,
            pct,
            p.elapsed.as_secs_f64()
        );
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
 
    fn on_finish(&mut self, results: &crate::stats::SimStats) {
        println!(); // close out the heartbeat line
        println!("=== Results ===");
        println!("wall time: {:.2}s", self.start_time.elapsed().as_secs_f64());
        print!("{}", results);
    }
}
