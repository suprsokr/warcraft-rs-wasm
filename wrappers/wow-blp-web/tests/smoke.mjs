// Smoke test for the wow-blp-web wasm bindings.
// Run after building pkg/ (see package-web job in ci.yml):
//   node wrappers/wow-blp-web/tests/smoke.mjs
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import assert from "node:assert/strict";

const pkg = join(dirname(fileURLToPath(import.meta.url)), "..", "pkg");
const { default: init, decodeBlp, encodeBlp, blpToPng, pngToBlp } = await import(
  join(pkg, "wow_blp_web.js")
);
await init({ module_or_path: readFileSync(join(pkg, "wow_blp_web_bg.wasm")) });

// --- build a 16x16 RGBA test image ---
const W = 16, H = 16;
const rgba = new Uint8Array(W * H * 4);
for (let y = 0; y < H; y++) {
  for (let x = 0; x < W; x++) {
    const i = (y * W + x) * 4;
    rgba[i] = x * 16;       // R gradient
    rgba[i + 1] = y * 16;   // G gradient
    rgba[i + 2] = 128;      // B constant
    rgba[i + 3] = 255;      // opaque
  }
}

// --- encodeBlp (raw3: lossless) -> decodeBlp round-trip ---
const blp = encodeBlp(rgba, W, H, { version: "blp2", format: "raw3", mipmaps: true });
assert.ok(blp.length > 0, "encodeBlp produced bytes");

const decoded = decodeBlp(blp);
assert.equal(decoded.width, W);
assert.equal(decoded.height, H);
assert.equal(decoded.version, "blp2");
assert.equal(decoded.compression, "raw3");
assert.ok([0, 1, 4, 8].includes(decoded.alphaBits), "alphaBits is a valid bit depth");
assert.ok(decoded.mipmaps.length > 1, "mipmaps were generated");
assert.equal(decoded.mipmaps[0].width, W);
assert.deepEqual([...decoded.rgba], [...rgba], "raw3 round-trip is lossless");

// --- mipmap level decoding ---
const mip1 = decodeBlp(blp, 1);
assert.equal(mip1.width, W / 2);
assert.equal(mip1.height, H / 2);
assert.equal(mip1.rgba.length, (W / 2) * (H / 2) * 4);

// --- dxt5 encode -> decode (lossy, just check shape) ---
const dxt = encodeBlp(rgba, W, H, { format: "dxt5", quality: "fast" });
const dxtDecoded = decodeBlp(dxt);
assert.equal(dxtDecoded.compression, "dxt5");
assert.equal(dxtDecoded.width, W);
assert.equal(dxtDecoded.rgba.length, W * H * 4);

// --- blp1 default (jpeg) encode -> decode ---
const blp1 = encodeBlp(rgba, W, H, { version: "blp1" });
assert.equal(decodeBlp(blp1).version, "blp1");

// --- blpToPng / pngToBlp round-trip ---
const png = blpToPng(blp);
assert.ok(png.length > 0, "blpToPng produced bytes");
assert.equal(png[0], 0x89, "PNG magic byte");
const blp2 = pngToBlp(png, { format: "raw3" });
const decoded2 = decodeBlp(blp2);
assert.equal(decoded2.width, W);
assert.equal(decoded2.height, H);
assert.deepEqual([...decoded2.rgba], [...rgba], "PNG round-trip is lossless");

// --- error handling ---
assert.throws(() => decodeBlp(new Uint8Array([1, 2, 3])), /failed to parse BLP/);
assert.throws(() => encodeBlp(new Uint8Array(8), W, H), /rgba length mismatch/);
assert.throws(() => encodeBlp(rgba, W, H, { format: "nope" }), /unknown format/);
assert.throws(() => encodeBlp(rgba, W, H, { version: "blp0" }), /blp1.*blp2/);
assert.throws(() => encodeBlp(rgba, W, H, { format: "dxt5", version: "blp1" }), /not supported for blp1/);

console.log("smoke test passed");
