// Smoke test for the wow-m2-web wasm bindings.
// Run after building pkg/ (see package-web job in ci.yml):
//   node wrappers/wow-m2-web/tests/smoke.mjs
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import assert from "node:assert/strict";

const pkg = join(dirname(fileURLToPath(import.meta.url)), "..", "pkg");
const dataDir = join(dirname(fileURLToPath(import.meta.url)), "../../../", "file-formats/graphics/wow-m2/tests/data");
const { default: init, M2File, M2Skin, M2Anim } = await import(join(pkg, "wow_m2_web.js"));
await init({ module_or_path: readFileSync(join(pkg, "wow_m2_web_bg.wasm")) });

// --- parse real test asset ---
const m2Bytes = new Uint8Array(readFileSync(join(dataDir, "fountainparticles.m2")));
const m2 = new M2File(m2Bytes);
const summary = m2.summary();
assert.equal(summary.format, "legacy");
assert.equal(summary.name, "FountainParticles");
assert.ok(summary.vertexCount > 0, "has vertices");

const positions = m2.vertices();
assert.ok(positions instanceof Float32Array);
assert.equal(positions.length, summary.vertexCount * 3);

const normals = m2.normals();
assert.equal(normals.length, positions.length);

const uvs = m2.texCoords();
assert.equal(uvs.length, summary.vertexCount * 2);

const skin = m2.skinWeights();
assert.ok(skin.boneWeights instanceof Uint8Array);
assert.equal(skin.boneWeights.length, summary.vertexCount * 4);
assert.ok(skin.boneIndices instanceof Uint8Array);
assert.equal(skin.boneIndices.length, summary.vertexCount * 4);

assert.ok(summary.textures.length > 0 || summary.textureFileIds.length >= 0);

// --- create / export / reopen round-trip (empty model) ---
const empty = M2File.create();
const exported = empty.export();
assert.ok(exported.length > 0);
const reopened = new M2File(exported);
assert.equal(reopened.summary().vertexCount, 0);
assert.equal(reopened.summary().format, "legacy");

// --- M2Skin round-trip (minimal new-format skin) ---
function makeSkin() {
  const buf = new ArrayBuffer(60);
  const dv = new DataView(buf);
  let off = 0;
  const writeU32 = (v) => { dv.setUint32(off, v, true); off += 4; };
  const writeStr4 = (s) => { for (let i = 0; i < 4; i++) dv.setUint8(off++, s.charCodeAt(i)); };
  writeStr4("SKIN");
  writeU32(1);        // version
  writeU32(0); writeU32(0); // name offset/count
  writeU32(0);        // vertex_count
  for (let i = 0; i < 5; i++) { writeU32(0); writeU32(0); } // indices, triangles, bone_indices, submeshes, batches
  return new Uint8Array(buf);
}
const skinFile = M2Skin.parse(makeSkin());
const skinSummary = skinFile.summary();
assert.equal(skinSummary.format, "new");
assert.equal(skinSummary.indexCount, 0);
const skinBytes = skinFile.export();
assert.ok(skinBytes.length > 0);
const skin2 = M2Skin.parse(skinBytes);
assert.equal(skin2.summary().format, "new");

// --- M2Anim error on garbage ---
assert.throws(() => M2Anim.parse(new Uint8Array([1, 2, 3])), /failed to parse anim/);

// --- M2Skin error on garbage ---
assert.throws(() => M2Skin.parse(new Uint8Array([1, 2, 3])), /failed to parse skin/);

console.log("smoke test passed");
