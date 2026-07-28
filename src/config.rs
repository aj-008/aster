//! Configuration loading and CLI parsing interface
//!
//! Handles TOML config files and command line arguments
//! exposing both as typed structs to the rest of the
//! simulator

use crate::error::AsterError;
use clap::{Parser};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

/// A trace-driven cache hierarchy simulator for ChampSim traces.
#[derive(Parser, Debug)]
#[command(
    name = "aster",
    version, 
    about,
    after_help = "\
EXAMPLES:
  aster trace.champsimtrace.xz -w 10M -s 50M
  aster trace.xz -w 10M -s 50M --config config/srrip.toml

  Instruction counts accept K/M/B suffixes and underscores: 50M, 50_000_000."
)]
pub struct Args {
    /// Trace file to simulate
    #[arg(value_name = "TRACE", value_parser = parse_trace)]
    pub trace: String,

    /// Instructions to simulate after warmup [K/M/B suffixes allowed]
    #[arg(short, long, value_name = "N", value_parser = parse_count)]
    pub simulation_instructions: usize,

    /// Instructions used to warm the hierarchy before statistics are collected
    #[arg(short, long, 
        value_name = "N",
        value_parser = parse_count,
        default_value = "0"
    )]
    pub warmup_instructions: usize,

    /// TOML file describing the cache hierarchy
    #[arg(
        short, long, 
        value_name = "FILE", 
        default_value = "config/default.toml",
        env = "ASTER_CONFIG"
    )]
    pub config: PathBuf,
}


/// Parses an instruction count, accepting `K`/`M`/`B` suffixes and `_`
/// separators (`50M`, `50_000_000`, `2B` are all valid).
fn parse_count(s: &str) -> Result<usize, String> {
    let t = s.trim();
    let (digits, mult) = if let Some(d) = t.strip_suffix(['K', 'k']) {
        (d, 1_000)
    } else if let Some(d) = t.strip_suffix(['M', 'm']) {
        (d, 1_000_000)
    } else if let Some(d) = t.strip_suffix(['B', 'b']) {
        (d, 1_000_000_000)
    } else {
        (t, 1)
    };

    let n: usize = digits
        .replace('_', "")
        .trim()
        .parse()
        .map_err(|_| format!("`{s}` is not a valid instruction count (try `50M`)"))?;

    n.checked_mul(mult)
        .ok_or_else(|| format!("`{s}` overflows the instruction counter"))
}

/// Rejects a trace path that doesn't exist
fn parse_trace(s: &str) -> Result<String, String> {
    let p = Path::new(s);
    if !p.is_file() {
        return Err(format!("trace file `{s}` does not exist"));
    }
    Ok(s.to_string())
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
    validate_cache_config("L1I", &config.l1i)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);


    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Args::command().debug_assert();
    }

    #[test]
    fn parse_count_handles_suffixes_and_separators() {
        assert_eq!(parse_count("50").unwrap(), 50);
        assert_eq!(parse_count("10K").unwrap(), 10_000);
        assert_eq!(parse_count("50m").unwrap(), 50_000_000);
        assert_eq!(parse_count("2B").unwrap(), 2_000_000_000);
        assert_eq!(parse_count("50_000_000").unwrap(), 50_000_000);
    }

    #[test]
    fn parse_count_rejects_overflow_and_garbage() {
        assert!(parse_count("").is_err());
        assert!(parse_count("50X").is_err());
        assert!(parse_count("-1").is_err());
        assert!(parse_count(&format!("{}B", usize::MAX)).is_err());
    }

    #[test]
    fn parse_trace_rejects_missing_file() {
        assert!(parse_trace("/nonexistent/aster/trace.xz").is_err());
    }




    /// Writes `contents` to a uniquely-named file in the OS temp dir and
    /// returns its path. No cleanup crate needed; caller may leave the
    /// file behind (temp dir is reaped by the OS).
    fn write_temp_file(name_hint: &str, contents: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "aster_config_test_{name_hint}_{}_{}.toml",
            std::process::id(),
            n
        ));
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        path
    }

    fn valid_cache_config() -> CacheConfig {
        CacheConfig {
            block_size: 64,
            cache_size: 32768,
            associativity: 8,
            replacement_policy: default_policy(),
            prefetcher: default_prefetcher(),
            repl_settings: default_repl_settings(),
            prefetch_settings: default_prefetch_settings(),
        }
    }

    const FULL_CONFIG_TOML: &str = r#"
        [llc]
        block_size = 64
        cache_size = 2097152
        associativity = 16

        [l2]
        block_size = 64
        cache_size = 524288
        associativity = 8

        [l1i]
        block_size = 64
        cache_size = 32768
        associativity = 4

        [l1d]
        block_size = 64
        cache_size = 32768
        associativity = 8
    "#;

    #[test]
    fn load_config_from_path_parses_valid_toml() {
        let path = write_temp_file("valid", FULL_CONFIG_TOML);
        let config = load_config_from_path(&path).unwrap();
        assert_eq!(config.llc.cache_size, 2097152);
        assert_eq!(config.l1i.associativity, 4);
    }

    #[test]
    fn load_config_from_path_rejects_malformed_toml() {
        let path = write_temp_file("malformed", "this is not [ valid toml");
        let err = load_config_from_path(&path).unwrap_err();
        assert_eq!(err.kind(), crate::error::ErrorKind::Config);
    }

    #[test]
    fn load_config_from_path_propagates_missing_file_as_io_error() {
        let missing = std::env::temp_dir().join("aster_config_test_does_not_exist.toml");
        let err = load_config_from_path(&missing).unwrap_err();
        assert_eq!(err.kind(), crate::error::ErrorKind::Io);
    }

    #[test]
    fn cache_config_missing_optional_fields_use_defaults() {
        let toml_str = "block_size = 64\ncache_size = 32768\nassociativity = 8\n";
        let cfg: CacheConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.replacement_policy, "lru");
        assert!(cfg.prefetcher.is_none());
        assert_eq!(cfg.repl_settings, default_repl_settings());
        assert_eq!(cfg.prefetch_settings, default_prefetch_settings());
    }

    #[test]
    fn default_repl_and_prefetch_settings_are_empty_tables() {
        assert_eq!(default_repl_settings(), toml::Value::Table(toml::map::Map::new()));
        assert_eq!(default_prefetch_settings(), toml::Value::Table(toml::map::Map::new()));
    }

    #[test]
    fn validate_cache_config_accepts_valid_geometry() {
        assert!(validate_cache_config("TEST", &valid_cache_config()).is_ok());
    }

    #[test]
    fn validate_cache_config_rejects_non_po2_block_size() {
        let mut cfg = valid_cache_config();
        cfg.block_size = 60;
        let err = validate_cache_config("TEST", &cfg).unwrap_err();
        assert!(err.to_string().contains("block_size"));
    }

    #[test]
    fn validate_cache_config_rejects_non_po2_associativity() {
        let mut cfg = valid_cache_config();
        cfg.associativity = 6;
        let err = validate_cache_config("TEST", &cfg).unwrap_err();
        assert!(err.to_string().contains("associativity"));
    }

    #[test]
    fn validate_cache_config_rejects_non_po2_cache_size() {
        let mut cfg = valid_cache_config();
        cfg.cache_size = 30000;
        let err = validate_cache_config("TEST", &cfg).unwrap_err();
        assert!(err.to_string().contains("cache_size"));
    }

    #[test]
    fn validate_cache_config_rejects_undersized_cache_relative_to_geometry() {
        // block_size * associativity (64*8=512) > cache_size (256): num_sets would be 0.
        let mut cfg = valid_cache_config();
        cfg.block_size = 64;
        cfg.associativity = 8;
        cfg.cache_size = 256;
        let err = validate_cache_config("TEST", &cfg).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("cache_size"));
    }

    #[test]
    fn validate_cache_config_boundary_block_times_assoc_equals_cache_size_is_ok() {
        let mut cfg = valid_cache_config();
        cfg.block_size = 64;
        cfg.associativity = 8;
        cfg.cache_size = 512; // exactly block_size * associativity -> num_sets == 1
        assert!(validate_cache_config("TEST", &cfg).is_ok());
    }
}
