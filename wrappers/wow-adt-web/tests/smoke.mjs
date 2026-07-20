// Smoke test for the wow-adt-web wasm bindings.
// Run after building pkg/ (see package-web job in ci.yml):
//   node wrappers/wow-adt-web/tests/smoke.mjs
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import assert from "node:assert/strict";

const pkg = join(dirname(fileURLToPath(import.meta.url)), "..", "pkg");
const { default: init, AdtFile } = await import(join(pkg, "wow_adt_web.js"));
await init({ module_or_path: readFileSync(join(pkg, "wow_adt_web_bg.wasm")) });

// --- create / export / reopen round-trip ---
const adt = AdtFile.create("tileset/generic.blp", "wotlk");
const bytes = adt.export();
assert.ok(bytes.length > 0, "export produced bytes");

const reopened = new AdtFile(bytes);
const summary = reopened.summary();
assert.equal(summary.version, "Wrath of the Lich King 3.x");
assert.deepEqual(summary.textures, ["tileset/generic.blp"]);
assert.equal(summary.terrainChunkCount, 256);
assert.equal(reopened.chunkCount(), 256);
assert.equal(summary.hasWater, false);
assert.equal(summary.hasFlightBounds, false);

// --- per-chunk accessors ---
const info = reopened.chunkInfo(0);
assert.equal(info.indexX, 0);
assert.equal(info.indexY, 0);
assert.equal(info.position.length, 3);
assert.equal(typeof info.holesLowRes, "number");
assert.equal(info.holesHighRes, undefined); // pre-MoP chunk

const hm = reopened.heightmap(0);
assert.ok(hm instanceof Float32Array, "heightmap is Float32Array");
assert.equal(hm.length, 145);
for (const v of hm) assert.equal(v, 0);

const layers = reopened.textureLayers(0);
assert.ok(Array.isArray(layers));
assert.ok(layers.length >= 1, "auto-generated chunk has a base layer");
assert.equal(layers[0].textureId, 0);

// out-of-range chunk
assert.throws(() => reopened.chunkInfo(256), /out of range/);

// --- loadSplit with resolver (no companions present → root only) ---
const resolverCalls = [];
const split = AdtFile.loadSplit("Azeroth_30_30.adt", bytes, (name) => {
  resolverCalls.push(name);
  return undefined;
});
assert.equal(split.summary().terrainChunkCount, 256);
assert.deepEqual(resolverCalls.sort(), [
  "Azeroth_30_30_lod.adt",
  "Azeroth_30_30_obj0.adt",
  "Azeroth_30_30_tex0.adt",
]);
// null is also accepted as "missing"
AdtFile.loadSplit("Azeroth_30_30.adt", bytes, () => null);

// --- error handling ---
assert.throws(() => new AdtFile(new Uint8Array([1, 2, 3])), /failed to parse ADT/);
assert.throws(() => AdtFile.create("tileset/generic.blp", "legion"), /invalid version/);

console.log("smoke test passed");
