# warcraft-rs-wasm

WebAssembly builds of the [warcraft-rs](https://github.com/wowemulation-dev/warcraft-rs)
file-format libraries. This repository is a **maintained fork**: the crates
below originate from upstream warcraft-rs but are developed here
independently, with wasm support (browser and WASI) as a first-class goal.

Each crate compiles for `wasm32-unknown-unknown` (browser / JS hosts) and
`wasm32-wasip1` (WASI runtimes), and selected crates additionally ship
ready-to-use **wasm-bindgen bindings** for JavaScript/TypeScript (see
[Web / JavaScript usage](#web--javascript-usage)).

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
started this fork; development continues here independently,
under the same MIT OR Apache-2.0 license.

## Using the crates (Rust)

There are no prebuilt binaries to download — and none are needed. These are
Rust libraries: Cargo compiles them from source for whatever target you
build for, including wasm. The fork is not on crates.io yet, so depend on
this repository directly:

```toml
[dependencies]
wow-mpq = { git = "https://github.com/suprsokr/warcraft-rs-wasm" }
```

(The upstream versions on crates.io work too, but only this fork guarantees
the wasm-compatible build and reader-based APIs.)

```sh
rustup target add wasm32-unknown-unknown   # or wasm32-wasip1
cargo build --target wasm32-unknown-unknown
```

That's it: if your crate (typically a `cdylib` using `wasm-bindgen`, or a
WASI binary) depends on these libraries, they are compiled into your `.wasm`
module automatically.

> Note: some optional features are not wasm-compatible. For example,
> `wow-mpq`'s `async` feature pulls in `tokio` filesystem APIs that only
> work on native targets. CI checks the default feature set against both
> `wasm32-unknown-unknown` and `wasm32-wasip1`.

## Web / JavaScript usage

For browser and Node.js/TypeScript consumers, wasm-bindgen wrapper crates
live under [`wrappers/`](wrappers/):

| Wrapper | Wraps | Status |
| ------- | ----- | ------ |
| [`wow-adt-web`](wrappers/wow-adt-web) | `wow-adt` | read/write tiles (monolithic + split sets) |
| [`wow-blp-web`](wrappers/wow-blp-web) | `wow-blp` | decode/encode + PNG helpers |
| [`wow-cdbc-web`](wrappers/wow-cdbc-web) | `wow-cdbc` | read/write DBC records with schema support |
| [`wow-m2-web`](wrappers/wow-m2-web) | `wow-m2` | read/write M2 models + skin/anim parsers |
| [`wow-wmo-web`](wrappers/wow-wmo-web) | `wow-wmo` | read WMO roots + group geometry |
| [`wow-mpq-web`](wrappers/wow-mpq-web) | `wow-mpq` | full read/write |
| [`wow-wdl-web`](wrappers/wow-wdl-web) | `wow-wdl` | full read/write |
| [`wow-wdt-web`](wrappers/wow-wdt-web) | `wow-wdt` | full read/write |

Each wrapper has its own README with API details and code examples (linked
in the table above).

Nothing is published to npm yet, so there are two ways to get the
bindings:

**Option 1 — download a prebuilt package.** Every
[GitHub release](https://github.com/suprsokr/warcraft-rs-wasm/releases)
attaches `<wrapper>-<version>-web.tar.gz` for each wrapper: the
ready-to-use wasm-bindgen output (`.js`, `.d.ts`, `.wasm`) plus
`package.json` and the wrapper's README. The `.wasm` binaries are
additionally optimized with `wasm-opt` to keep download sizes small.
Unpack it into your project and import it directly:

```sh
tar xzf wow-mpq-web-*-web.tar.gz   # creates ./pkg/
```

**Option 2 — build it yourself:**

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli

cargo build --release -p wow-mpq-web --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir wrappers/wow-mpq-web/pkg \
  target/wasm32-unknown-unknown/release/wow_mpq_web.wasm
```

Either way, import the ES module from your app (via a bundler, or plain
browser modules with `await init()`). A minimal example — see each
wrapper's README for its full API:

```js
import init, { MpqArchive } from "./pkg/wow_mpq_web.js";
await init();

const archive = new MpqArchive(new Uint8Array(await file.arrayBuffer()));
console.log(archive.list());                // [{name, size, ...}, ...]
const data = archive.readFile("patch.m2");  // Uint8Array
```

Notes:

- Everything happens in memory: bytes in (`Uint8Array`), bytes out. No
  filesystem access is needed, so this works in any browser.

## What this repository is for

- Maintaining the forked crates with wasm (`wasm32-unknown-unknown` and
  `wasm32-wasip1`) as a supported target.
- Providing JS/TS bindings (wasm-bindgen) for use in web apps.
- CI that continuously verifies both wasm targets, the wasm-bindgen
  wrappers (with a Node smoke test), and native tests.

Nothing is published to crates.io or npm yet: Rust consumers should use git
dependencies (`wow-mpq = { git = "...", ... }`), and JS consumers either
download the prebuilt web packages attached to
[GitHub Releases](https://github.com/suprsokr/warcraft-rs-wasm/releases) or
build the bindings locally.

## License

MIT OR Apache-2.0, same as upstream warcraft-rs.
