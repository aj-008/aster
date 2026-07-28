# Aster

[![CI](https://github.com/aj-008/aster/actions/workflows/ci.yml/badge.svg)](https://github.com/aj-008/aster/actions/workflows/ci.yml)

A trace-driven cache hierarchy simulator for ChampSim traces, written in Rust.

Aster models a four-level hierarchy (L1I, L1D, L2, LLC) with configurable
geometry, pluggable replacement policies, and pluggable prefetchers. It is built
for studying cache replacement and prefetching behaviour.

**What it is not:** Aster has no timing model and no core model. It consumes an
instruction/memory trace and reports cache behaviour.
It does not produce cycle counts or IPC.

## Install

Requires Rust 1.85 or newer.

```sh
cargo install --git https://github.com/aj-008/aster
```

Or build from source:

```sh
git clone https://github.com/aj-008/aster
cd aster
cargo build --release   # binary at target/release/aster
```

## Usage

```sh
aster <TRACE> -s <N> [-w <N>] [--config <FILE>]
```

```sh
# 20M instructions, no warmup, default hierarchy
aster traces/429.mcf-217B.champsimtrace.trace.gz -s 20M

# warmup for 10M instructions first, custom config
aster traces/429.mcf-217B.champsimtrace.trace.gz -w 10M -s 50M --config config/srrip.toml
```

Instruction counts accept `K`/`M`/`B` suffixes and underscore separators, so
`50M` and `50_000_000` are equivalent. Simulation instructions are counted
after warmup completes.

| Flag | Description | Default |
| --- | --- | --- |
| `<TRACE>` | ChampSim trace file (positional, required) | — |
| `-s`, `--simulation-instructions` | Instructions to simulate after warmup | required |
| `-w`, `--warmup-instructions` | Instructions used to warm the hierarchy | `0` |
| `-c`, `--config` | TOML file describing the hierarchy | `config/default.toml` |

`--config` can also be set through the `ASTER_CONFIG` environment variable.

Traces are the standard ChampSim format.

## Configuration

The hierarchy is described by a TOML file with one table per cache level. Sizes
are in bytes; `block_size`, `cache_size`, and `associativity` must all be powers
of two.

```toml
[llc]
block_size = 64
cache_size = 2097152
associativity = 16
replacement_policy = "srrip"

[llc.repl_settings]
max_rrpv = 8
insertion_rrpv = 1
increment = 3

[l2]
block_size = 64
cache_size = 524288
associativity = 8
replacement_policy = "srrip"
prefetcher = "stream_buffer"

[l2.prefetch_settings]
degree = 4
num_streams = 10

[l1i]
block_size = 64
cache_size = 32768
associativity = 4

[l1d]
block_size = 64
cache_size = 32768
associativity = 8
```

`replacement_policy` defaults to `lru` and `prefetcher` defaults to none, so both
may be omitted. Policy- and prefetcher-specific knobs live in the
`repl_settings` and `prefetch_settings` sub-tables; any omitted key falls back to
that component's own default.

## Supported components

### Replacement policies

| Name | Description |
| --- | --- |
| `lru` | Least-recently-used |
| `srrip` | Static re-reference interval prediction |

### Prefetchers

| Name | Description |
| --- | --- |
| `stream_buffer` | PC-keyed stream buffer |


## Future Work

- Validation metrics against ChampSim
- JSON statistics output for scripted sweeps
- Shell completion generation
- Additional replacement policies (DRRIP, SHiP, MockingJay)
- Per-level policy overrides from the command line

## Contributing

Issues and pull requests are welcome. Before opening a PR, please run:

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
