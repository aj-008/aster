use std::cmp::min;

use crate::{policy::ReplacementPolicy, trace_reader::MemAccess, error::AsterError};
use serde::Deserialize;

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
        value.clone().try_into().map_err(|e| AsterError::InvalidPolicyConfig(e.to_string()))
    }
}

fn default_max_rrpv() -> u8 { 3 }
fn default_insertion_rrpv() -> u8 { 2 }
fn default_increment() -> u8 { 1 }


pub struct Srrip {
    max_rrpv: u8,
    insertion_rrpv: u8,
    increment: u8,
    rrpv_values: Vec<Vec<u8>>,
}


impl Srrip {
    pub fn new(cache_size: usize, block_size: usize, associativity: usize, settings: &toml::Value) -> Result<Self, AsterError> {
        // this should really never fail since there are default values for all fields
        let s: SrripSettings = settings.try_into()?;
        let num_sets = cache_size / (block_size * associativity);
        
        // ideally check that insertion is less than max rrpv
        
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


    // worth noting that 'find_victim' only increments 
    // once, not until a line reaches 'max_rrpv'
    fn find_victim(&mut self, set: usize) -> usize {
        let victim_set: &Vec<u8> = self.rrpv_values.get(set).expect("set index out of bounds");
        let (victim_index, victim_rrpv) = match victim_set
            .iter()
            .enumerate()
            .max_by_key(|(_, item)| *item) {
            Some((idx, rrpv)) => (idx, *rrpv),
            None => (0, *victim_set.first().expect("set index failure (associativity is 0)")),
        };

        if victim_rrpv < self.max_rrpv {
            for rrpv in self.rrpv_values[set].iter_mut() {
                *rrpv = min(*rrpv + self.increment, self.max_rrpv);
            }
        }

        victim_index
    }

    fn install(&mut self, set: usize, way: usize, access: &MemAccess) {
        self.update(set, way, access);
    }

}
