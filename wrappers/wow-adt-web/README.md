# wow-adt-web

Read and **edit** World of Warcraft **ADT terrain tiles** in the browser and
Node.js — WebAssembly bindings for the [`wow-adt`](../../file-formats/world-data/wow-adt)
Rust crate. ADT files are the 16×16 chunk terrain tiles of a map: heightmaps,
texture layers, alpha maps, liquid, and object placements. Everything happens
in memory.

Terrain tiles are large, so the API is two-level: `summary()` returns cheap
metadata (version, texture/model/WMO name lists, counts), while heavy
per-chunk data is fetched on demand (`heightmap()`, `alphaMap()`).

Full mutating API: edit heightmaps, textures, alpha maps, liquid, and object
placements in memory, then `export()` back to bytes.

## Getting the package

Nothing is published to npm yet. Either download
`wow-adt-web-<version>-web.tar.gz` from the
[GitHub releases](https://github.com/suprsokr/warcraft-rs-wasm/releases)
and unpack it (`tar xzf ...` creates `./pkg/`), or build it yourself:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli

cargo build --release -p wow-adt-web --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir wrappers/wow-adt-web/pkg \
  target/wasm32-unknown-unknown/release/wow_adt_web.wasm
```

## Usage

```js
import init, { AdtFile } from "./pkg/wow_adt_web.js";
await init();

// Monolithic (pre-Cataclysm) tile — or the root file of a split set:
const adt = new AdtFile(new Uint8Array(await file.arrayBuffer()));

adt.summary();
// {
//   version, terrainChunkCount,
//   textures: [...], models: [...], wmos: [...],
//   doodadPlacementCount, wmoPlacementCount,
//   hasWater, hasFlightBounds, hasTextureFlags, hasBlendMeshes,
// }

// Per-chunk data (index 0..255):
adt.chunkInfo(0);
// {
//   indexX, indexY, position: [x, y, z], areaId, flags,
//   holesLowRes, holesHighRes /* string | undefined (MoP 5.3+) */,
//   layerCount, hasHeights, hasNormals, hasAlpha, hasShadow,
//   hasVertexColors, hasLiquid,
// }
adt.heightmap(0);      // Float32Array(145) — 9×9 outer + 8×8 inner,
                       // relative to chunkInfo(0).position[1]
adt.textureLayers(0);  // [{textureId, flags, offsetInMcal, effectId}, ...]
adt.alphaMap(0);       // Uint8Array with raw MCAL data, or undefined

// Serialize back to a monolithic root ADT:
const bytes = adt.export();

// Creating from scratch (256 empty terrain chunks, one base texture):
const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
```

### Cataclysm+ split files

Cataclysm split ADTs into a root file plus `_tex0`/`_obj0` (and Legion+
`_lod`) companions. `loadSplit` merges them, resolving companion files
lazily through a callback — which composes naturally with
[`wow-mpq-web`](../wow-mpq-web):

```js
const adt = AdtFile.loadSplit(
  "World/Maps/Azeroth/Azeroth_30_30.adt",
  rootBytes,
  (name) => (archive.hasFile(name) ? archive.readFile(name) : undefined),
);
```

The resolver is called with `<stem>_tex0.adt`, `<stem>_obj0.adt`, and
`<stem>_lod.adt`; return `undefined`/`null` for files that don't exist.

## Editing

All mutations happen in memory; call `export()` to serialize back to a
monolithic root ADT `Uint8Array`. The builder handles offset computation
and index maintenance automatically.

### Heightmap

```js
adt.setHeightmap(0, new Float32Array(145));    // write MCVT heights, clear normals
adt.heightmap(0);                               // Float32Array(145)
adt.setChunkInfo(0, {                           // mutate MCNK header fields
  areaId: 42,
  holesLowRes: 0x000F,
  position: new Float32Array([x, y, z]),
});
```

### Textures & layers

```js
const id = adt.addTexture("terrain/dirt.blp");  // add to MTEX, returns index
adt.removeTexture(id);                           // remove, fix MCLY references

adt.addTextureLayer(0, {                         // add MCLY entry
  textureId: 1, flags: 0x100, effectId: 0
});
adt.removeTextureLayer(0, 1);                    // remove by layer index

adt.setAlphaMap(0, alphaUint8Array);             // raw MCAL for a chunk
adt.alphaMap(0);                                 // Uint8Array or undefined
```

### WMO & M2 placements

```js
adt.addWmo("buildings/house.wmo");               // add to MWMO, returns nameId
adt.addWmoPlacement({
  nameId: 0, uniqueId: 1,
  position: new Float32Array([x, y, z]),
  rotation: new Float32Array([rx, ry, rz]),
  extentsMin: new Float32Array([x, y, z]),
  extentsMax: new Float32Array([x, y, z]),
  flags: 0, doodadSet: 0, nameSet: 0,
});
adt.removeWmoPlacement(index);                   // remove MODF entry

adt.addModel("doodads/tree.m2");                 // add to MMDX, returns nameId
adt.addDoodadPlacement({                          // add MDDF entry
  nameId: 0, uniqueId: 42,
  position: new Float32Array([x, y, z]),
  rotation: new Float32Array([rx, ry, rz]),
  scale: 1024,  // 1024 = 1.0x
});
adt.removeDoodadPlacement(index);
```

### Water / Liquid

**Vanilla/TBC** (MCLQ per chunk):

```js
// vertices: Float32Array(162) = 81 pairs of [depthUnion, height]
adt.setMclq(0, liquidType, minHeight, maxHeight, verts);
adt.clearMclq(0);
```

**WotLK** (MH2O root-level):

```js
adt.setMh2o(0, {
  liquidType: 2, minHeight: 0, maxHeight: 10,
  width: 8, height: 8,
  vertexData: new Float32Array(81),         // optional height grid
  fishableBitmap: "0", deepBitmap: "0",     // optional u64 as string
});
adt.clearMh2oEntry(0);
```

### Terrain brushes

Low-level helpers for the JS UI to build brushes on top of:

```js
adt.changeTerrain(idx, x, y, radius, delta); // raise/lower brush
adt.flattenTerrain(idx, targetHeight, radius);
adt.smoothTerrain(idx, radius);
adt.clearHeight(idx);                         // zero out MCVT
adt.setHoles(idx, x, y, add);                 // edit 4×4 hole map
adt.recalcNormals();                          // recompute all MCNR
adt.recalcChunkNormals(idx);                  // recompute one chunk's MCNR
adt.fixGaps();                                // match edge vertices across chunks
```

## Notes

- Passing a `_tex0`/`_obj0`/`_lod` file directly to `new AdtFile(...)` is
  an error pointing you at `loadSplit`.
- `holesHighRes` is serialized as a decimal string because JS numbers
  cannot hold a u64 exactly.
- ADT versions are detected from content (chunk presence), not from a
  version field — `summary().version` reflects the detected version.
