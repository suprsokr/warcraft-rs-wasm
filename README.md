# warcraft-rs-wasm

WebAssembly targets of the [warcraft-rs](https://github.com/wowemulation-dev/warcraft-rs)
file-format libraries: each crate below is the upstream library, ported to
compile for `wasm32-unknown-unknown` (browser / JS hosts) and
`wasm32-wasip1` (WASI runtimes).

## Crates

| Crate | Upstream source | crates.io |
| ----- | --------------- | --------- |
| `wow-mpq` | [file-formats/archives/wow-mpq](https://github.com/wowemulation-dev/warcraft-rs/tree/main/file-formats/archives/wow-mpq) | [crates.io/crates/wow-mpq](https://crates.io/crates/wow-mpq) |
| `wow-adt` | [file-formats/world-data/wow-adt](https://github.com/wowemulation-dev/warcraft-rs/tree/main/file-formats/world-data/wow-adt) | [crates.io/crates/wow-adt](https://crates.io/crates/wow-adt) |
| `wow-wdl` | [file-formats/world-data/wow-wdl](https://github.com/wowemulation-dev/warcraft-rs/tree/main/file-formats/world-data/wow-wdl) | [crates.io/crates/wow-wdl](https://crates.io/crates/wow-wdl) |
| `wow-wdt` | [file-formats/world-data/wow-wdt](https://github.com/wowemulation-dev/warcraft-rs/tree/main/file-formats/world-data/wow-wdt) | [crates.io/crates/wow-wdt](https://crates.io/crates/wow-wdt) |
| `wow-blp` | [file-formats/graphics/wow-blp](https://github.com/wowemulation-dev/warcraft-rs/tree/main/file-formats/graphics/wow-blp) | [crates.io/crates/wow-blp](https://crates.io/crates/wow-blp) |
| `wow-m2` | [file-formats/graphics/wow-m2](https://github.com/wowemulation-dev/warcraft-rs/tree/main/file-formats/graphics/wow-m2) | [crates.io/crates/wow-m2](https://crates.io/crates/wow-m2) |
| `wow-wmo` | [file-formats/graphics/wow-wmo](https://github.com/wowemulation-dev/warcraft-rs/tree/main/file-formats/graphics/wow-wmo) | [crates.io/crates/wow-wmo](https://crates.io/crates/wow-wmo) |
| `wow-cdbc` | [file-formats/database/wow-cdbc](https://github.com/wowemulation-dev/warcraft-rs/tree/main/file-formats/database/wow-cdbc) | [crates.io/crates/wow-cdbc](https://crates.io/crates/wow-cdbc) |

## Credits

All credit for the libraries themselves goes to the
[warcraft-rs authors and contributors](https://github.com/wowemulation-dev/warcraft-rs/graphs/contributors)
— Daniel S. Reichenbach and the wowemulation-dev team. This repository only
adds a wasm build target and CI/release packaging on top of their work,
under the same MIT OR Apache-2.0 license.

## Building

```sh
rustup target add wasm32-unknown-unknown wasm32-wasip1

# Browser / JS hosts
cargo build --release --workspace --target wasm32-unknown-unknown

# WASI runtimes
cargo build --release --workspace --target wasm32-wasip1
```

These are Rust libraries (`rlib`s): link them into your own `cdylib` wasm
module, or use them natively. GitHub Releases (created on `v*` tags) attach
per-target tarballs of the compiled libraries.

## License

MIT OR Apache-2.0, same as upstream warcraft-rs.
