use aster::config::load_config;
use aster::error::AsterError;
use aster::simulator::Simulator;

fn main() -> Result<(), AsterError> {
    let (config, args) = load_config()?;
    let mut sim = Simulator::new(config, args)?;
    sim.run()?;
    Ok(())
}
