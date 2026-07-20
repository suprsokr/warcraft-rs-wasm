# wow-wdt-web

Read and write World of Warcraft **WDT map table files** in the browser
and Node.js — WebAssembly bindings for the [`wow-wdt`](../../file-formats/world-data/wow-wdt)
Rust crate. WDT files describe which tiles of a map exist and hold
optional global WMO placement. Everything happens in memory.

## Getting the package

Nothing is published to npm yet. Either download
`wow-wdt-web-<version>-web.tar.gz` from the
[GitHub releases](https://github.com/suprsokr/warcraft-rs-wasm/releases)
and unpack it (`tar xzf ...` creates `./pkg/`), or build it yourself:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli

cargo build --release -p wow-wdt-web --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir wrappers/wow-wdt-web/pkg \
  target/wasm32-unknown-unknown/release/wow_wdt_web.wasm
```

## Usage

```js
import init, { WdtFile } from "./pkg/wow_wdt_web.js";
await init();

// The WDT format is version-dependent, so a version is required:
// an expansion name ("classic", "tbc", "wotlk", "cata", "mop", "wod",
// "legion", "bfa", "shadowlands", "dragonflight") or a version string
// ("1.12.1", "3.3.5a", ...).
const wdt = new WdtFile(new Uint8Array(await file.arrayBuffer()), "3.3.5a");

wdt.summary();
// {
//   version, isWmoOnly, mphdFlags, tileCount, hasMaid,
//   wmoFilenames: [...], wmoPlacements: [...],
//   validationWarnings: [...],   // version-aware structural checks
// }

wdt.tiles();                 // [{x, y, hasAdt, areaId, flags}, ...] existing tiles
wdt.getTile(32, 32);         // single tile, or undefined if out of range

// Mutate and re-export
wdt.setTile(33, 32, true, 42);        // (x, y, hasAdt, areaId?)
wdt.addWmoFilename("World\\wmo\\dungeon.wmo");  // for WMO-only maps
const bytes = wdt.export();           // Uint8Array

// Creating from scratch
const fresh = WdtFile.create("wotlk");
```

Note: `wow-wdt` re-detects the format version from content when reading
(chunk presence rules), so `summary().version` reflects the *detected*
version, which may differ from the hint passed to the constructor (e.g. a
terrain map without an MWMO chunk is detected as Cataclysm+).
