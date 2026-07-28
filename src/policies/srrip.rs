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

        if s.max_rrpv < s.increment + s.insertion_rrpv {
            return Err(AsterError::Config(
                "SrripSettings: max_rrpv must be greater than increment + insertion_rrpv"
                    .to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(table: &[(&str, i64)]) -> toml::Value {
        let mut map = toml::map::Map::new();
        for (k, v) in table {
            map.insert(k.to_string(), toml::Value::Integer(*v));
        }
        toml::Value::Table(map)
    }

    fn empty_settings() -> toml::Value {
        toml::Value::Table(toml::map::Map::new())
    }

    fn hit() -> MemAccess {
        MemAccess {
            addr: 0,
            pc: 0,
            is_write: false,
            hit: Some(true),
        }
    }

    fn miss() -> MemAccess {
        MemAccess {
            addr: 0,
            pc: 0,
            is_write: false,
            hit: Some(false),
        }
    }

    #[test]
    fn default_settings_construct_successfully() {
        // max_rrpv=3, insertion_rrpv=2, increment=1: 3 >= 1+2, so this must pass.
        assert!(Srrip::new(32768, 64, 8, &empty_settings()).is_ok());
    }

    #[test]
    fn rejects_settings_where_max_rrpv_leaves_no_aging_room() {
        // max_rrpv (2) < increment (1) + insertion_rrpv (2) -> invalid.
        let s = settings(&[("max_rrpv", 2), ("insertion_rrpv", 2), ("increment", 1)]);
        assert!(Srrip::new(32768, 64, 8, &s).is_err());
    }

    #[test]
    fn accepts_settings_at_the_validation_boundary() {
        // max_rrpv exactly equal to increment + insertion_rrpv is accepted (not "<").
        let s = settings(&[("max_rrpv", 3), ("insertion_rrpv", 2), ("increment", 1)]);
        assert!(Srrip::new(32768, 64, 8, &s).is_ok());
    }

    #[test]
    fn hit_resets_rrpv_to_zero() {
        let mut srrip = Srrip::new(32768, 64, 4, &empty_settings()).unwrap();
        srrip.update(0, 0, &hit());
        assert_eq!(srrip.rrpv_values[0][0], 0);
    }

    #[test]
    fn miss_inserts_at_insertion_rrpv() {
        let s = settings(&[("max_rrpv", 3), ("insertion_rrpv", 2), ("increment", 1)]);
        let mut srrip = Srrip::new(32768, 64, 4, &s).unwrap();
        srrip.update(0, 0, &miss());
        assert_eq!(srrip.rrpv_values[0][0], 2);
    }

    #[test]
    fn install_behaves_like_update() {
        let mut srrip = Srrip::new(32768, 64, 4, &empty_settings()).unwrap();
        srrip.install(0, 0, &hit());
        assert_eq!(srrip.rrpv_values[0][0], 0);
    }

    #[test]
    fn find_victim_picks_existing_max_rrpv_line_without_aging() {
        let mut srrip = Srrip::new(32768, 64, 3, &empty_settings()).unwrap();
        // [0, 3, 1] -- way 1 is already at max_rrpv (3), so no aging pass is needed.
        srrip.rrpv_values[0] = vec![0, 3, 1];
        assert_eq!(srrip.find_victim(0), 1);
        assert_eq!(
            srrip.rrpv_values[0],
            vec![0, 3, 1],
            "no line should have aged"
        );
    }

    #[test]
    fn find_victim_ages_all_lines_until_one_reaches_max_rrpv() {
        // Standard SRRIP: repeatedly age every line by `increment` (capped at
        // max_rrpv) until some line reaches max_rrpv, then evict it. Starting
        // at [0, 1, 0] with max_rrpv=3, increment=1: no line is at 3, so age
        // once -> [1, 2, 1] (still none at 3), age again -> [2, 3, 2]. Way 1
        // is now at max_rrpv and is evicted; the *other* lines are left at 2,
        // not 1 -- this is the textbook aging behavior (Jaleel et al.), not
        // the single-increment shortcut the code used before this test suite
        // was written.
        let mut srrip = Srrip::new(32768, 64, 3, &empty_settings()).unwrap();
        srrip.rrpv_values[0] = vec![0, 1, 0];
        assert_eq!(srrip.find_victim(0), 1);
        assert_eq!(srrip.rrpv_values[0], vec![2, 3, 2]);
    }

    #[test]
    fn find_victim_stops_aging_at_max_rrpv_cap() {
        let mut srrip = Srrip::new(32768, 64, 2, &empty_settings()).unwrap();
        srrip.rrpv_values[0] = vec![0, 0];
        // Neither line starts at max_rrpv (3); repeated aging must cap at 3,
        // not overflow past it.
        let victim = srrip.find_victim(0);
        assert!(srrip.rrpv_values[0][victim] == 3);
        assert!(srrip.rrpv_values[0].iter().all(|&v| v <= 3));
    }

    #[test]
    fn sets_are_independent() {
        let mut srrip = Srrip::new(65536, 64, 2, &empty_settings()).unwrap();
        srrip.rrpv_values[0] = vec![3, 0];
        srrip.rrpv_values[1] = vec![0, 3];
        assert_eq!(srrip.find_victim(0), 0);
        assert_eq!(srrip.find_victim(1), 1);
    }
}
