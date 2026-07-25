use std::cmp::min;

use crate::{error::AsterError, policy::ReplacementPolicy, trace_reader::MemAccess};
use serde::Deserialize;

/// TOML-deserialized tunables for [`Srrip`]; all fields default if omitted.
#[derive(Deserialize)]
struct SrripSettings {
    #[serde(default = "default_max_rrpv")]
    max_rrpv: u8,
    #[serde(default = "default_insertion_rrpv")]
    insertion_rrpv: u8,
    #[serde(default = "default_increment")]
    increment: u8,
}

impl TryFrom<&toml::Value> for SrripSettings {
    type Error = AsterError;
    fn try_from(value: &toml::Value) -> Result<Self, Self::Error> {
        value
            .clone()
            .try_into()
            .map_err(|e| AsterError::InvalidPolicyConfig(e.to_string()))
    }
}

fn default_max_rrpv() -> u8 {
    3
}
fn default_insertion_rrpv() -> u8 {
    2
}
fn default_increment() -> u8 {
    1
}

/// Static re-reference interval prediction (SRRIP) replacement policy.
/// Tracks a per-way re-reference prediction value (RRPV) per set; hits
/// reset RRPV to 0 (near-immediate re-reference), misses insert at
/// `insertion_rrpv`, and eviction prefers the way predicted furthest from
/// re-reference (highest RRPV).
pub struct Srrip {
    max_rrpv: u8,
    insertion_rrpv: u8,
    increment: u8,
    rrpv_values: Vec<Vec<u8>>,
}

impl Srrip {
    /// Builds an `Srrip` state for a cache with the given geometry,
    /// deserializing `settings` into `SrripSettings` (missing fields fall
    /// back to defaults).
    ///
    /// # Errors
    /// Returns [`AsterError::InvalidPolicyConfig`] if `settings` doesn't
    /// deserialize into `SrripSettings`.
    pub fn new(
        cache_size: usize,
        block_size: usize,
        associativity: usize,
        settings: &toml::Value,
    ) -> Result<Self, AsterError> {
        // try_into should really never fail since there are default values for all fields
        let s: SrripSettings = settings.try_into()?;
        let num_sets = cache_size / (block_size * associativity);

        if s.max_rrpv > s.increment {
           return Err(AsterError::Config(
                "SrripSettings: increment must be less than max rrpv".to_string(),
            ));
        }

        Ok(Self {
            rrpv_values: vec![vec![s.insertion_rrpv; associativity]; num_sets],
            max_rrpv: s.max_rrpv,
            insertion_rrpv: s.insertion_rrpv,
            increment: s.increment,
        })
    }
}

impl ReplacementPolicy for Srrip {
    fn update(&mut self, set: usize, way: usize, access: &MemAccess) {
        if access.hit.unwrap_or(false) {
            self.rrpv_values[set][way] = 0;
        } else {
            self.rrpv_values[set][way] = self.insertion_rrpv;
        }
    }

    fn find_victim(&mut self, set: usize) -> usize {
        loop {
            let victim_set = self.rrpv_values.get(set).expect("set index out of bounds");

            // find the first line with max rrpv if it exists,
            // otherwise age all lines
            if let Some((idx, _)) = victim_set
                .iter()
                .enumerate()
                .find(|&(_, &rrpv)| rrpv == self.max_rrpv)
            {
                return idx;
            }
            for rrpv in self.rrpv_values[set].iter_mut() {
                *rrpv = min(*rrpv + self.increment, self.max_rrpv);
            }
        }
    }

    fn install(&mut self, set: usize, way: usize, access: &MemAccess) {
        self.update(set, way, access);
    }
}
