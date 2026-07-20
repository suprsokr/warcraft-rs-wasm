# wow-wmo-web

Read and write World of Warcraft **WMO world model files** in the browser and
Node.js — WebAssembly bindings for the [`wow-wmo`](../../file-formats/graphics/wow-wmo)
Rust crate. A WMO is one root file plus N group files (`name.wmo`,
`name_000.wmo`, …). Everything happens in memory.

Group geometry (the heavy part) is loaded lazily through a resolver
callback and exposed as typed arrays.

## Getting the package

Install from npm (recommended for JS/TS):

```sh
npm install wow-wmo-web
```

Or download `wow-wmo-web-<version>-web.tar.gz` from the
[GitHub releases](https://github.com/suprsokr/warcraft-rs-wasm/releases)
and unpack it (`tar xzf ...` creates `./pkg/`), or build it yourself:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli

cargo build --release -p wow-wmo-web --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir wrappers/wow-wmo-web/pkg \
  target/wasm32-unknown-unknown/release/wow_wmo_web.wasm
```

## Usage

```js
import init, { WmoFile } from "wow-wmo-web";
// If you downloaded the tarball or built locally, use:
// import init, { WmoFile } from "./pkg/wow_wmo_web.js";
await init();

// Monolithic root-only (group files not loaded)
const wmo = new WmoFile(new Uint8Array(await file.arrayBuffer()));

wmo.summary();
// {
//   version, groupCount, loadedGroupCount,
//   textures: [...], materialCount, portalCount,
//   lightCount, doodadDefCount, doodadSetCount,
//   boundingBoxMin, boundingBoxMax,
//   skybox, groups: [{index, name, flags, loaded, vertexCount, triangleCount}, ...]
// }

wmo.addTexture("tileset/dungeon.blp");
const rootBytes = wmo.exportRoot();          // Uint8Array

// Load with groups (Cataclysm+ or legacy multi-file)
const wmo2 = WmoFile.loadWithGroups("dungeon.wmo", rootBytes, (name) =>
  archive.hasFile(name) ? archive.readFile(name) : undefined,
);
const verts = wmo2.groupVertices(0);        // Float32Array (x,y,z interleaved)
const normals = wmo2.groupNormals(0);       // Float32Array
const uvs = wmo2.groupTexCoords(0);         // Float32Array (u,v interleaved)
const indices = wmo2.groupIndices(0);       // Uint16Array

// Creating from scratch
const fresh = WmoFile.create("wotlk");
fresh.addTexture("tileset/dungeon.blp");
```

## Notes

- `loadWithGroups` derives group filenames from the root name using the
  conventional `<stem>_000.wmo`, `<stem>_001.wmo`, … pattern.
- Group files are parsed read-only; only the root file is writable.
- The resolver is called for every expected group file. Returning
  `undefined`/`null` for a missing file is an error (unlike optional
  split ADT companions, groups are required when listed by the root).
