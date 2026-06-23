use aster::config::load_config;
use aster::simulator::Simulator;
use aster::trace_reader::TraceReader;


/// main is a function
fn main() {
    let (config, args) = load_config();
    let reader = TraceReader::from_path(&args.trace)
        .expect("erm, check trace_reader");
    let sim = Simulator::new(config, args);
    sim.run(reader);
}
