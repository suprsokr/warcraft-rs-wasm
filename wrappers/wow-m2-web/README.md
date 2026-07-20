# wow-m2-web

Read and write World of Warcraft **M2 model files** in the browser and Node.js —
WebAssembly bindings for the [`wow-m2`](../../file-formats/graphics/wow-m2)
Rust crate. Everything happens in memory.

M2 models reference external `.skin` and `.anim` files; those are parsed
by separate classes (`M2Skin`, `M2Anim`) from bytes the caller supplies.
This keeps the library fully in-memory and composes naturally with
[`wow-mpq-web`](../wow-mpq-web).

## Getting the package

Install from npm (recommended for JS/TS):

```sh
npm install wow-m2-web
```

Or download `wow-m2-web-<version>-web.tar.gz` from the
[GitHub releases](https://github.com/suprsokr/warcraft-rs-wasm/releases)
and unpack it (`tar xzf ...` creates `./pkg/`), or build it yourself:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli

cargo build --release -p wow-m2-web --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir wrappers/wow-m2-web/pkg \
  target/wasm32-unknown-unknown/release/wow_m2_web.wasm
```

## Usage

```js
import init, { M2File, M2Skin } from "wow-m2-web";
// If you downloaded the tarball or built locally, use:
// import init, { M2File, M2Skin } from "./pkg/wow_m2_web.js";
await init();

const model = new M2File(new Uint8Array(await file.arrayBuffer()));

model.summary();
// {
//   name, format: "legacy" | "chunked", version (raw header value),
//   vertexCount, boneCount, animationCount, textureCount, materialCount,
//   particleEmitterCount, ribbonEmitterCount,
//   textures: [{textureType, flags, filename}, ...],
//   skinFileIds: [...], animationFileIds: [...], textureFileIds: [...]
// }

const positions = model.vertices();   // Float32Array (x,y,z interleaved)
const normals = model.normals();      // Float32Array
const uvs = model.texCoords();        // Float32Array (u,v interleaved)
const skin = model.skinWeights();     // { boneWeights: Uint8Array, boneIndices: Uint8Array }

// Re-serialize
const bytes = model.export();

// External .skin file
const skinFile = M2Skin.parse(archive.readFile("model00.skin"));
skinFile.summary();  // { format, indexCount, triangleCount, submeshCount, batchCount }
const tris = skinFile.triangles();  // Uint16Array
const skinBytes = skinFile.export();

// External .anim file
import { M2Anim } from "./pkg/wow_m2_web.js";
const anim = M2Anim.parse(archive.readFile("model.anim"));
anim.summary();  // { format, sectionCount }
const animBytes = anim.export();
```

## Notes

- The companion-file resolver is intentionally left to JS — this gives
  you full control over caching, async loading, MPQ lookups, etc.
- `M2File.export()` re-serializes the main model only. Skin and anim
  files are separate and must be exported via `M2Skin.export()` /
  `M2Anim.export()` if you mutated them.
