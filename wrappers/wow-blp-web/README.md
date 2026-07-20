# wow-blp-web

Decode and encode World of Warcraft **BLP textures** in the browser and
Node.js — WebAssembly bindings for the [`wow-blp`](../../file-formats/graphics/wow-blp)
Rust crate. Everything happens in memory: bytes in (`Uint8Array`),
pixels/bytes out.

Supports BLP1 (Warcraft III) and BLP2 (WoW) with all encodings: palettized
(RAW1), uncompressed RGBA (RAW3), JPEG, and DXT1/3/5.

## Getting the package

Install from npm (recommended for JS/TS):

```sh
npm install wow-blp-web
```

Or download `wow-blp-web-<version>-web.tar.gz` from the
[GitHub releases](https://github.com/suprsokr/warcraft-rs-wasm/releases)
and unpack it (`tar xzf ...` creates `./pkg/`), or build it yourself:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli

cargo build --release -p wow-blp-web --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir wrappers/wow-blp-web/pkg \
  target/wasm32-unknown-unknown/release/wow_blp_web.wasm
```

## Usage

```js
import init, { decodeBlp, encodeBlp, blpToPng, pngToBlp } from "wow-blp-web";
// If you downloaded the tarball or built locally, use:
// import init, { decodeBlp, encodeBlp, blpToPng, pngToBlp } from "./pkg/wow_blp_web.js";
await init();

// Decode: metadata + RGBA pixels of one mipmap level (default 0)
const tex = decodeBlp(new Uint8Array(await file.arrayBuffer()));
// {
//   width, height,
//   rgba: Uint8Array,          // width * height * 4 RGBA bytes
//   mipmaps: [{level, width, height, data_size}, ...],
//   version: "blp1" | "blp2",
//   compression: "jpeg" | "raw1" | "raw3" | "dxt1" | "dxt3" | "dxt5",
//   alphaBits: 0 | 1 | 4 | 8,
// }
const mip2 = decodeBlp(bytes, 2);             // optional mipmap level

// Encode: raw RGBA pixels -> BLP bytes
const blp = encodeBlp(tex.rgba, tex.width, tex.height, {
  version: "blp2",        // "blp1" | "blp2"          (default "blp2")
  format: "dxt5",         // see below               (default "dxt5" for blp2, "jpeg" for blp1)
  alphaBits: 8,           // 0|1|4|8, "raw1" only    (default 8)
  hasAlpha: true,         // jpeg/dxt* only          (default true)
  quality: "balanced",    // "fast"|"balanced"|"best", DXTn only (default "balanced")
  mipmaps: true,          // generate mipmaps        (default true)
  filter: "triangle",     // mipmap filter: "nearest"|"triangle"|"catmullRom"|"gaussian"|"lanczos3"
});

// PNG convenience helpers (input can be PNG, JPEG, BMP, TGA, ...)
const png = blpToPng(bytes);                  // BLP -> PNG bytes
const back = pngToBlp(png, { format: "dxt1" }); // image -> BLP bytes
```

Format availability: `"raw1"` and `"jpeg"` work for both versions;
`"raw3"`, `"dxt1"`, `"dxt3"`, `"dxt5"` are BLP2 only.

**Limitation:** BLP0 files with *external* mipmap files (Warcraft III RoC
beta) cannot be decoded by this in-memory API. BLP1/BLP2 always embed
their mipmaps, so this rarely matters in practice.
