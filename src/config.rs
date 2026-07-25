//! Configuration loading and CLI parsing interface
//!
//! Handles TOML config files and command line arguments
//! exposing both as typed structs to the rest of the
//! simulator

use crate::error::AsterError;
use clap::Parser;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

/// CLI arguments parsed by clap
#[derive(Parser)]
#[command(version, about)]
pub struct Args {
    #[arg(short, long, default_value = "default.toml")]
    pub config: PathBuf,

    #[arg(short, long, required = true)]
    pub trace: String,

    #[arg(short, long, required = true)]
    pub simulation_instructions: usize,

    #[arg(short, long, required = true)]
    pub warmup_instructions: usize,
}

/// Typed config struct parsed from TOML file
#[derive(Deserialize, Debug)]
pub struct Config {
    pub llc: CacheConfig,
    pub l2: CacheConfig,
    pub l1i: CacheConfig,
    pub l1d: CacheConfig,
}

/// Cache-specific configuration
#[derive(Deserialize, Debug)]
pub struct CacheConfig {
    pub block_size: usize,
    pub cache_size: usize,
    pub associativity: usize,
    #[serde(default = "default_policy")]
    pub replacement_policy: String,
    #[serde(default = "default_prefetcher")]
    pub prefetcher: Option<String>,
    #[serde(default = "default_repl_settings")]
    pub repl_settings: toml::Value,
    #[serde(default = "default_prefetch_settings")]
    pub prefetch_settings: toml::Value,
}

/// Parses command line arguments via clap and
/// loads config via 'load_config_from_path'
///
/// # Errors
/// Returns [`AsterError::Io`] if the file cannot be read,
/// or [`AsterError::Config`] if the TOML is malformed
pub fn load_config() -> Result<(Config, Args), AsterError> {
    let args = Args::parse();
    let config = load_config_from_path(&args.config)?;

    validate_cache_config("LLC", &config.llc)?;
    validate_cache_config("L2", &config.l2)?;
    validate_cache_config("L1D", &config.l1d)?;

    Ok((config, args))
}

/// Default `repl_settings` value (empty table) used when a `CacheConfig`
/// omits policy-specific settings; each policy then falls back to its own
/// field defaults.
pub fn default_repl_settings() -> toml::Value {
    toml::Value::Table(toml::map::Map::new())
}

pub fn default_prefetch_settings() -> toml::Value {
    toml::Value::Table(toml::map::Map::new())
}

fn default_policy() -> String {
    "lru".to_string()
}

fn default_prefetcher() -> Option<String> {
    None
}

/// Loads a [`Config`] from a TOML file at the path `path`
///
/// # Errors
/// Returns [`AsterError::Io`] if the file cannot be read,
/// or [`AsterError::Config`] if the TOML is malformed
///
/// # Examples
/// ```ex_run
/// use std::path::Path;
/// use aster::config::load_config_from_path;
/// let config = load_config_from_path(Path::new("config/default.toml")).unwrap();
/// ```
pub fn load_config_from_path(path: &std::path::Path) -> Result<Config, AsterError> {
    toml::from_str(&fs::read_to_string(path)?)
        .map_err(|err| AsterError::Config(format!("Failed to parse TOML config: {}", err)))
}

/// Confirms the user input valid Po2 values for the
/// cache parameters
///
/// # Errors
/// Returns [`AsterError::Io`] if the cache parameters are invalid
pub fn validate_cache_config(name: &str, cfg: &CacheConfig) -> Result<(), AsterError> {
    if !cfg.block_size.is_power_of_two() {
        return Err(AsterError::Config(format!(
            "{}: block_size must be a power of two",
            name
        )));
    }
    if !cfg.associativity.is_power_of_two() {
        return Err(AsterError::Config(format!(
            "{}: associativity must be a power of two",
            name
        )));
    }
    if !cfg.cache_size.is_power_of_two() {
        return Err(AsterError::Config(format!(
            "{}: cache_size must be a power of two",
            name
        )));
    }
    if cfg.block_size * cfg.associativity > cfg.cache_size {
        return Err(AsterError::Config(format!(
            "{}: cache_size must be larger than BS * assoc.",
            name
        )));
    }
    Ok(())
}

pub fn validate_options(args: &Args) -> Result<(), AsterError> {
    if args.warmup_instructions > args.simulation_instructions {
        return Err(AsterError::Config(
                "Warmup instructions must be less than simulation instructions".to_string()
        ));
    }
    Ok(())
}
