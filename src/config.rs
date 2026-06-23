use serde::Deserialize;
use std::path::PathBuf;
use clap::Parser;
use std::fs;
use crate::error::AsterError;

#[derive(Parser)]
#[command(version, about)]
pub struct Args {
    #[arg(short, long, default_value = "default.toml")]
    pub config: PathBuf,

    #[arg(short, long, required=true)]
    pub trace: String,

    #[arg(short, long, required=true)]
    pub simulation_instructions: usize,

    #[arg(short, long, required=true)]
    pub warmup_instructions: usize,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub llc: CacheConfig,
    pub l2: CacheConfig,
    pub l1i: CacheConfig,
    pub l1d: CacheConfig,
}

#[derive(Deserialize, Debug)]
pub struct CacheConfig {
    pub block_size: usize,
    pub cache_size: usize,
    pub associativity: usize,
    pub replacement_policy: Option<String>,
}


pub fn load_config() -> Result<(Config, Args), AsterError> {
    let args = Args::parse();
    let config = load_config_from_path(&args.config)?;
    Ok((config, args))
}
 
pub fn load_config_from_path(path: &std::path::Path) -> Result<Config, AsterError> {
    toml::from_str(&fs::read_to_string(path)?)
        .map_err(|err| AsterError::Config(format!("Failed to parse TOML config: {}", err)))
}
