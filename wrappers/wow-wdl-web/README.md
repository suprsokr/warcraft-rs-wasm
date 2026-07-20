# wow-wdl-web

[![npm](https://img.shields.io/npm/v/wow-wdl-web)](https://www.npmjs.com/package/wow-wdl-web)

Read and write World of Warcraft **WDL low-resolution terrain files** in
the browser and Node.js — WebAssembly bindings for the
[`wow-wdl`](../../file-formats/world-data/wow-wdl) Rust crate. WDL files
store 17x17 low-res heightmaps per map tile for distant terrain rendering
and the world map. Everything happens in memory.

## Getting the package

Install from npm (recommended for JS/TS):

```sh
npm install wow-wdl-web
```

Or download `wow-wdl-web-<version>-web.tar.gz` from the
[GitHub releases](https://github.com/suprsokr/warcraft-rs-wasm/releases)
and unpack it (`tar xzf ...` creates `./pkg/`), or build it yourself:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli

cargo build --release -p wow-wdl-web --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir wrappers/wow-wdl-web/pkg \
  target/wasm32-unknown-unknown/release/wow_wdl_web.wasm
```

## Usage

```js
import init, { WdlFile } from "wow-wdl-web";
// If you downloaded the tarball or built locally, use:
// import init, { WdlFile } from "./pkg/wow_wdl_web.js";
await init();

const wdl = new WdlFile(new Uint8Array(await file.arrayBuffer()));
// Optional version hint: new WdlFile(bytes, "wotlk"). All WDL versions
// share the same MVER number, so the hint controls which optional chunks
// (MWMO pre-Legion / ML** Legion+) are expected. Default "latest".

wdl.summary();               // { version, versionNumber, tileCount, holesCount, wmoFilenames }
wdl.tiles();                 // [{x, y}, ...] tiles that have heightmap data

const hm = wdl.heightmap(32, 32);
// { outer: Int16Array(289), inner: Int16Array(256) }  — 17x17 corner +
// 16x16 center heights; undefined if the tile has no data
const holes = wdl.holes(32, 32);   // Uint16Array(16) row bitmasks, or undefined

// Mutate and re-export (MAOF offsets are recomputed on write)
wdl.setHeightmap(32, 32, hm.outer, hm.inner);
wdl.setHoles(32, 32, new Uint16Array(16));
wdl.removeTile(10, 20);
const bytes = wdl.export();        // Uint8Array

// Creating from scratch (expansion name: "vanilla", "wotlk", "cata",
// "mop", "wod", "legion", "bfa", "shadowlands", "dragonflight", "latest")
const fresh = WdlFile.create("wotlk");
```
