# warcraft-rs-wasm

WebAssembly builds of the [warcraft-rs](https://github.com/wowemulation-dev/warcraft-rs)
file-format libraries.

This repository contains the wasm-compatible library crates from warcraft-rs,
set up as a standalone workspace that compiles to:

- `wasm32-unknown-unknown` (browser / JS hosts)
- `wasm32-wasip1` (WASI runtimes)

## Crates

| Crate | Description |
| ----- | ----------- |
| `wow-mpq` | MPQ archive reading/writing |
| `wow-adt` | ADT terrain tiles |
| `wow-wdl` | WDL low-resolution world data |
| `wow-wdt` | WDT world map tables |
| `wow-blp` | BLP texture images |
| `wow-m2` | M2 models |
| `wow-wmo` | WMO world map objects |
| `wow-cdbc` | DBC/DB2 database files |

## Building

```sh
rustup target add wasm32-unknown-unknown wasm32-wasip1

# Browser/JS hosts
cargo build --release --workspace --target wasm32-unknown-unknown

# WASI runtimes
cargo build --release --workspace --target wasm32-wasip1
```

## Notes for wasm consumers

- These crates are Rust libraries (`rlib`s): you link them into your own
  `cdylib` wasm module (or use them natively). Release artifacts attach
  prebuilt rlibs for both targets.
- `wow-mpq`'s optional `async` (tokio `fs`) and `mmap` (libc) features are
  not available on `wasm32-unknown-unknown`.
- `wow-mpq`'s `test_utils` module and its `rand` dependency are disabled on
  wasm targets.
- `rayon`-based parallel APIs compile for wasm, but calling them in a
  browser requires a wasm threads setup; they work under WASI with the
  appropriate thread support.

## Releases

Tags (`v*`) trigger a GitHub Actions release that builds both wasm targets
and attaches per-target tarballs with the compiled libraries.

## License

MIT OR Apache-2.0, same as upstream warcraft-rs.
