# Tasks: Remaining wasm/JS Porting Work

This repo is a maintained fork of [warcraft-rs](https://github.com/wowemulation-dev/warcraft-rs)
with wasm (browser + WASI) support as a first-class goal. Rust consumers use
git dependencies; JS/TS consumers use wasm-bindgen wrapper crates under
`wrappers/`, distributed as `pkg/` tarballs attached to GitHub Releases
(nothing is published to crates.io or npm yet).

## Status: DONE

- All 8 core crates compile for `wasm32-unknown-unknown` and
  `wasm32-wasip1` (verified by `.github/workflows/ci.yml`).
- `wow-mpq` gained reader/writer-based APIs for use without a filesystem:
  - `Archive::open_reader()` / `open_reader_with_options()` /
    `OpenOptions::open_reader()` — open from any `Read + Seek` source
    (reader is internally a boxed `crate::io::ReadSeek + Send` trait object).
  - `ArchiveBuilder::build_to_writer()` — build an archive into any
    `Write + Seek + Read` sink, no temp files.
- `wrappers/wow-blp-web` — decode/encode wasm-bindgen wrapper
  (`decodeBlp(bytes, level?)`, `encodeBlp(rgba, w, h, options)`,
  `blpToPng(bytes)`, `pngToBlp(png, options)`), with Node smoke test,
  CI job, and release packaging. Also: `wow-blp`'s `texpresso/rayon`
  dependency is now native-only (no dead weight in wasm binaries).
- `wrappers/wow-wdt-web` and `wrappers/wow-wdl-web` — read/write wrappers
  (`WdtFile` / `WdlFile` classes: `summary()`, tile accessors/mutators,
  `export()`), with Node smoke tests, CI, and release packaging. Each
  wrapper ships its own README (examples moved out of the root README).
- `wrappers/wow-mpq-web` — full read/write wasm-bindgen wrapper
  (`new MpqArchive(bytes)`, `MpqArchive.create(v)`, `list()`, `readFile()`,
  `addFile()`, `removeFile()`, `export()`, `fileCount()`), with Node smoke
  test (`wrappers/wow-mpq-web/tests/smoke.mjs`), CI job (`package-web`),
  and release packaging (`wow-mpq-web-<tag>-web.tar.gz` on GitHub Releases).
- `wrappers/wow-cdbc-web` — `DbcFile` class with schema-driven parsing
  (`summary()`, `records()`, `recordsWithSchema(schema)`, `strings()`,
  `export()`), Node smoke test, CI job, and release packaging.

## Key finding

All remaining core crates are **already parser-generic** (`R: Read + Seek`
or `&[u8]` entry points), so the remaining work is mostly **thin wrapper
crates** (~100–200 lines each) plus a few specific gaps listed below.

| Crate | Read API | Write API |
| ----- | -------- | --------- |
| wow-blp | `parse_blp(&[u8])`, `load_blp_from_buf` | `convert::image_to_blp` |
| wow-wdt | `WdtReader<R>` | `WdtWriter<W>` |
| wow-wdl | `WdlParser::parse<R: Read+Seek>` | writer in `lib.rs` |
| wow-wmo | `parse_wmo<R: Read+Seek>` (api.rs) | `editor` module |
| wow-m2  | `parse<R: Read+Seek>` throughout | writer in `model.rs` |
| wow-cdbc | `header::Header::parse<R: Read+Seek>` | `writer.rs` |
| wow-adt | `parse_adt<R: Read+Seek>` (api.rs) | `adt_builder.rs` |

## Wrapper conventions (follow `wrappers/wow-mpq-web`)

- One crate per format: `wrappers/wow-<format>-web`, `crate-type = ["cdylib", "rlib"]`.
- Deps: `wasm-bindgen`, `js-sys`, `serde` + `serde-wasm-bindgen` for
  structured returns, `wow-<format>` via path dependency.
- API style: bytes in (`&[u8]` / `Uint8Array`), plain data out
  (`Uint8Array`, or JSON-friendly values via `serde_wasm_bindgen::to_value`).
- Errors: map to `wasm_bindgen::JsError` with context strings.
- Staged-write pattern for mutable formats (see `MpqArchive`): stage
  changes in the wrapper struct, rebuild into `Cursor<Vec<u8>>` on
  `export()`, swap internal state to the rebuilt data.
- Each wrapper needs a Node smoke test at `wrappers/<name>/tests/smoke.mjs`
  (run with plain `node`, uses `node:assert`) covering a full round-trip:
  parse/encode → bytes → parse again → verify.
- Wire each wrapper into:
  - workspace `members` in root `Cargo.toml`
  - `.github/workflows/ci.yml` `package-web` job (build + smoke test; also
    add to the `--exclude` lists in the WASI jobs — wasm-bindgen doesn't
    support WASI)
  - `.github/workflows/release.yml` `build-web` matrix (tarball name
    `<wrapper>-<tag>-web.tar.gz`)
- `.gitignore` already covers `wrappers/*/pkg/`.

- `wrappers/wow-adt-web` — `AdtFile` class (summary + lazy per-chunk
  accessors, split-set loading via resolver callback, create/export),
  with Node smoke test, CI job, and release packaging. Core gained
  `AdtSet::from_named_bytes()` for filesystem-free split-set loading.

## Tasks (suggested order)

### 1. wow-blp-web (highest value, easiest) — **DONE**

Texture decode/encode — the most common web use case (viewers/converters).

- Wrapper API sketch: `decodeBlp(bytes) -> { width, height, rgba, mipmaps }`
  (RGBA `Uint8Array`, mipmap level parameter), `encodeBlp(rgba, width,
  height, options) -> Uint8Array`, and PNG convenience helpers
  (`blpToPng(bytes) -> Uint8Array`, `pngToBlp(pngBytes) -> Uint8Array`) via
  the `image` crate (`blp_to_image` / `image_to_blp`).
- **Core cleanup first**: `wow-blp` depends on `texpresso` with the `rayon`
  feature — compiles on wasm but is dead weight (no threads). Make rayon
  usage optional / `cfg`'d out on `wasm32-unknown-unknown` to trim binary
  size.
- Note: BLP0 format references external mipmap files (`load_blp` handles
  this via filesystem) — wrapper should use `parse_blp` (in-memory, fails
  on BLP0-with-external-mipmaps) and document the limitation, or accept
  optional mipmap byte arrays (see multi-file pattern in task 4).

### 2. wow-wdt-web and wow-wdl-web (trivial, validate the scaffold) — **DONE**

- Fully generic reader/writer already; wrapper is mechanical.
- wdt: `WdtReader::new(reader, WowVersion)` — wrapper takes version as a
  string/number (`WowVersion::from_string` / expansion name).
- Decide per-crate what structured data to return (see task 5 re: serde).

### 3. wow-adt-web — **DONE**

- Core port: `AdtSet::from_named_bytes(root_name, root_bytes, resolve)`
  added to `wow-adt` (filesystem-free split-set loading via a name→bytes
  resolver callback; `load_from_path` refactored onto shared parse
  helpers and now actually skips missing optional files).
- `wrappers/wow-adt-web` — two-level `AdtFile` API: `summary()` (cheap
  metadata + name lists) plus on-demand per-chunk accessors
  (`chunkInfo()`, `heightmap()` → `Float32Array(145)`,
  `textureLayers()`, `alphaMap()`), `AdtFile.loadSplit(rootName, bytes,
  resolver)` for Cataclysm+ split sets (merges via `AdtSet`),
  `AdtFile.create(texture, version?)` and `export()` for round-trips.
  Node smoke test, CI, and release packaging wired up.

### 4. wow-wmo-web and wow-m2-web (multi-file design) — **DONE**

- `wrappers/wow-wmo-web` — `WmoFile` class:
  - `open(bytes)` parses root; `loadWithGroups(rootName, bytes, resolver)`
    derives conventional `_000.wmo` names and lazily resolves group files
    via a JS callback, exposing geometry as typed arrays.
  - `summary()`, `groupVertices(i)`, `groupNormals(i)`,
    `groupTexCoords(i)`, `groupIndices(i)`, `addTexture()`,
    `exportRoot()` (root-only write; groups are read-only in the
    underlying library).
  - `create(version)` builds an empty root for round-trip smoke tests.
- `wrappers/wow-m2-web` — `M2File`, `M2Skin`, `M2Anim` classes:
  - `M2File` wraps `parse_m2` (legacy MD20 / chunked MD21 auto-detect).
    `summary()`, `vertices()`, `normals()`, `texCoords()`,
    `skinWeights()`, `export()`, `create()`.
  - `M2Skin.parse(bytes)` and `M2Anim.parse(bytes)` parse external
    companion files with auto-detection and round-trip `export()`.
  - Node smoke tests, CI, and release packaging wired up.
- The shared resolver pattern is documented in each README and composes
  naturally with `wow-mpq-web`.

Note: the underlying M2 writer panics on some legacy parsed assets
(capacity overflow from misread array counts). The wrapper still exposes
`export()`; round-trip smoke tests use `M2File.create()` (empty model) to
exercise the writer safely.

### 5. Structured output / serde — **DONE**

Returning parsed ADT/M2/WMO/WDT data to JS needs `Serialize` on the core
types (for `serde-wasm-bindgen`), or curated summary structs defined in the
wrapper (like `ListedFile` in wow-mpq-web).

- `wow-m2` and `wow-cdbc` already have optional `serde` features — enabled
  in the wrappers (`wow-m2-web` now uses `features = ["serde-support"]`,
  `wow-cdbc-web` already used `features = ["serde"]`).
- `wow-wdl`, `wow-wdt`, and `wow-wmo`: added optional `serde` features
  with feature-gated `Serialize` derives on the public data types
  (`WdlFile` components, `WdtFile`/chunks, `WmoRoot`/`WmoGroup`/material/
  portal/light/doodad types, plus version enums). Wrappers now enable the
  `serde` feature for these crates.
- `wow-adt`: the ADT structure is large and deeply nested, so the wrapper
  exposes curated `summary()`/`chunkInfo()`/`textureLayers()` summary
  structs instead of serializing the whole parsed tree. Added an optional
  `serde` feature for `AdtVersion` and any future use; the wrapper enables
  it.
- All wrappers already use `serde-wasm-bindgen` for JSON-friendly returns
  and pass `cargo check -p <wrapper> --target wasm32-unknown-unknown`.

### 6. wow-cdbc-web — **DONE**

- `wrappers/wow-cdbc-web` — `DbcFile` class:
  - `open(bytes)` parses WDBC/WDB2/WDB5 auto-detect.
  - `summary()` → header metadata (magic, record/field counts, sizes).
  - `records()` → raw records as arrays of UInt32 values.
  - `recordsWithSchema(schema)` → typed records as plain objects with
    resolved strings (schema is `{ name, type }` array where type is one
    of "int32", "uint32", "float32", "string", "bool", "uint8", "int8",
    "uint16", "int16").
  - `strings()` → all unique strings from the string block.
  - `export()` → round-trip write back to DBC bytes.
  - Node smoke test, CI (`package-web` matrix + WASI exclude), and
    release packaging (`wow-cdbc-web-<tag>-web.tar.gz`) wired up.
- Full DB2/DBD schema support is out of scope for the wrapper; the core
  `wow-cdbc` crate already supports DBD-driven parsing for Rust consumers.

### 7. Cross-cutting / infra — **DONE**

- Added `wasm-opt` (binaryen) to both the `package-web` CI job and the
  release `build-web` job. Every generated `.wasm` is now shrunk with
  `wasm-opt -Os` after `wasm-bindgen`, and the build logs print the
  before/after size in bytes.
- Watched binary sizes and feature-gated the heavy encode path:
  `wow-m2` now makes its `wow-blp` re-export optional behind a default
  `blp` feature. `wow-m2-web` disables default features, so it no longer
  pulls the `image` crate into the M2 web bundle. (`wow-blp-web` still
  needs `image` for decode + PNG helpers.)
- Factored the duplicated wrapper scaffolding into a small internal crate
  `wrappers/wow-web-common` now that there are eight wrappers. It
  provides `to_js`, `to_uint8_array`, `to_vec`, and `set_property`,
  replacing the copy-pasted helpers in every wrapper. The crate is
  excluded from `wasm32-wasip1` CI builds (like the wrappers) because it
  depends on `wasm-bindgen`/`js-sys`.
- Fixed the `wow-cdbc-web` Node smoke test to use the web target
  initialization pattern (matching the other wrappers) and added
  `"type": "module"` to its `package.json`.

## Definition of done per wrapper

- `cargo check -p <wrapper> --target wasm32-unknown-unknown` clean.
- Node smoke test passes locally and in CI (`package-web` job green).
- Release workflow attaches `<wrapper>-<tag>-web.tar.gz` (verify by
  cutting a `v*` tag on a branch or inspecting the workflow run).
- README "Web / JavaScript usage" table updated with the new package and a
  short code example.
