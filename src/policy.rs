//! LLC replacement policy interface
//!
//! To add a new policy, include
//! `use crate::policies::policy_name::Policy;`
//! in the below import block, and update the
//! `make_policy` method to call the constructor.
//!
//! All new policies must implement the `ReplacementPolicy` trait.

use crate::{
    config::CacheConfig,
    error::AsterError,
    policies::{lru::Lru, srrip::Srrip},
    trace_reader::MemAccess,
};

/// Per-set, per-way replacement state for a cache. Implementors track
/// whatever bookkeeping their policy needs (timestamps, RRPV bits, etc.)
/// and are driven by `Cache::access` on every hit, install, and eviction.
pub trait ReplacementPolicy {
    /// Called on a cache hit to update state for the accessed way.
    fn update(&mut self, set: usize, way: usize, access: &MemAccess);
    /// Called on a miss (once no invalid way is available) to pick the way
    /// to evict from `set`.
    fn find_victim(&mut self, set: usize) -> usize;
    /// Called on a miss, after the victim way has been overwritten, to
    /// initialize state for the newly filled line. Policies that only
    /// implement `update` and skip this hook will never initialize state
    /// for freshly installed lines.
    fn install(&mut self, set: usize, way: usize, access: &MemAccess);
}

/// Constructs the [`ReplacementPolicy`] named by `config.replacement_policy`.
///
/// # Errors
/// Returns [`AsterError::InvalidPolicyConfig`] if the policy name is
/// unrecognized or `repl_settings` fails to deserialize into the policy's
/// settings type.
pub fn make_policy(config: &CacheConfig) -> Result<Box<dyn ReplacementPolicy>, AsterError> {
    match config.replacement_policy.as_str() {
        "lru" => Ok(Box::new(Lru::new(
            config.cache_size,
            config.block_size,
            config.associativity,
            &config.repl_settings,
        )?)),
        "srrip" => Ok(Box::new(Srrip::new(
            config.cache_size,
            config.block_size,
            config.associativity,
            &config.repl_settings,
        )?)),
        _ => Err(AsterError::InvalidPolicyConfig(
            "Unimplimented Policy".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_prefetch_settings, default_repl_settings};

    fn cfg(policy: &str) -> CacheConfig {
        CacheConfig {
            block_size: 64,
            cache_size: 32768,
            associativity: 8,
            replacement_policy: policy.to_string(),
            prefetcher: None,
            repl_settings: default_repl_settings(),
            prefetch_settings: default_prefetch_settings(),
        }
    }

    #[test]
    fn make_policy_builds_lru() {
        assert!(make_policy(&cfg("lru")).is_ok());
    }

    #[test]
    fn make_policy_builds_srrip_with_default_settings() {
        assert!(make_policy(&cfg("srrip")).is_ok());
    }

    #[test]
    fn make_policy_rejects_unknown_name() {
        match make_policy(&cfg("not_a_real_policy")) {
            Err(e) => assert_eq!(e.kind(), crate::error::ErrorKind::InvalidPolicyConfig),
            Ok(_) => panic!("expected an error for an unrecognized policy name"),
        }
    }
}
