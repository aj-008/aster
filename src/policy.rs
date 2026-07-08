//! LLC replacement policy interface
//!
//! To add a new policy, include 
//! `use crate::policies::policy_name::Policy;`
//! in the below import block, and update the 
//! `make_policy` method to call the constructor.
//!
//! All new policies must implement the `ReplacementPolicy` trait.

use crate::{config::CacheConfig, error::AsterError, policies::{lru::Lru, srrip::Srrip}, trace_reader::MemAccess};

pub trait ReplacementPolicy {
    fn update(&mut self, set: usize, way: usize, access: &MemAccess);
    fn find_victim(&mut self, set: usize) -> usize;
    fn install(&mut self, set: usize, way: usize, access: &MemAccess);
}

pub fn make_policy(config: &CacheConfig) -> Result<Box<dyn ReplacementPolicy>, AsterError> {
    match config.replacement_policy.as_str() {
        "lru" => Ok(Box::new(Lru::new(config.cache_size, config.block_size, config.associativity, &config.repl_settings)?)),
        "srrip" => Ok(Box::new(Srrip::new(config.cache_size, config.block_size, config.associativity, &config.repl_settings)?)),
        _ => Err(AsterError::InvalidPolicyConfig("Unimplimented Policy".to_string())),
    }
}
