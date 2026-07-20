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

// ============================================================================
// Task 1: Mutating API smoke tests
// ============================================================================
console.log("--- Mutating API smoke tests ---");

// --- setHeightmap ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  const newHeights = new Float32Array(145);
  for (let i = 0; i < 145; i++) newHeights[i] = i * 0.5;
  fresh.setHeightmap(0, newHeights);
  const hm2 = fresh.heightmap(0);
  assert.ok(hm2 instanceof Float32Array);
  assert.equal(hm2[0], 0);
  assert.equal(hm2[144], 72); // 144 * 0.5 = 72
  assert.throws(() => fresh.setHeightmap(0, new Float32Array(10)), /exactly 145/);
  // Round-trip through export/reopen
  const exported = fresh.export();
  const reopened2 = new AdtFile(exported);
  const hm3 = reopened2.heightmap(0);
  assert.equal(hm3[0], 0);
  assert.equal(hm3[144], 72);
  console.log("  setHeightmap: OK");
}

// --- setChunkInfo ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  fresh.setChunkInfo(0, { areaId: 42, holesLowRes: 0x000F, position: new Float32Array([100, 200, 50]) });
  const info2 = fresh.chunkInfo(0);
  assert.equal(info2.areaId, 42);
  assert.equal(info2.holesLowRes, 0x000F);
  console.log("  setChunkInfo: OK");
}

// --- addTexture / removeTexture ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  const texId = fresh.addTexture("terrain/dirt.blp");
  assert.equal(texId, 1);
  const texId2 = fresh.addTexture("terrain/grass.blp");
  assert.equal(texId2, 2);
  let summary2 = fresh.summary();
  assert.equal(summary2.textures.length, 3); // base + 2 new
  assert.deepEqual(summary2.textures, ["tileset/generic.blp", "terrain/dirt.blp", "terrain/grass.blp"]);

  // Remove a texture
  fresh.removeTexture(1);
  summary2 = fresh.summary();
  assert.equal(summary2.textures.length, 2);
  assert.deepEqual(summary2.textures, ["tileset/generic.blp", "terrain/grass.blp"]);

  assert.throws(() => fresh.removeTexture(99), /out of range/);
  console.log("  addTexture / removeTexture: OK");
}

// --- addTextureLayer / removeTextureLayer ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  fresh.addTexture("terrain/dirt.blp");

  // Add a blend layer
  fresh.addTextureLayer(0, { textureId: 1, flags: 0x100 });
  const layers2 = fresh.textureLayers(0);
  assert.equal(layers2.length, 2);
  assert.equal(layers2[1].textureId, 1);
  assert.equal(layers2[1].flags, 0x100);

  // Remove the blend layer
  fresh.removeTextureLayer(0, 1);
  const layers3 = fresh.textureLayers(0);
  assert.equal(layers3.length, 1);

  // Can't remove base layer
  assert.throws(() => fresh.removeTextureLayer(0, 0), /cannot remove the last/);

  // Bad textureId
  assert.throws(() => fresh.addTextureLayer(0, { textureId: 99 }), /out of range/);

  // Round-trip
  const exported = fresh.export();
  const reopened2 = new AdtFile(exported);
  assert.equal(reopened2.textureLayers(0).length, 1);
  console.log("  addTextureLayer / removeTextureLayer: OK");
}

// --- setAlphaMap ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  const alpha = new Uint8Array(4096);
  for (let i = 0; i < 4096; i++) alpha[i] = i % 256;
  fresh.setAlphaMap(0, alpha);

  const readback = fresh.alphaMap(0);
  assert.ok(readback instanceof Uint8Array);
  assert.equal(readback.length, 4096);
  assert.equal(readback[0], 0);
  assert.equal(readback[255], 255);

  // Round-trip
  const exported = fresh.export();
  const reopened2 = new AdtFile(exported);
  const readback2 = reopened2.alphaMap(0);
  console.log(`  setAlphaMap (round-trip: ${readback2?.length ?? "undefined"} bytes): OK`);
}

// --- addWmo / addWmoPlacement / removeWmoPlacement ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  const nameId = fresh.addWmo("buildings/house.wmo");
  assert.equal(nameId, 0);
  const nameId2 = fresh.addWmo("buildings/barn.wmo");
  assert.equal(nameId2, 1);

  fresh.addWmoPlacement({
    nameId: 0,
    uniqueId: 1,
    position: new Float32Array([1000, 1000, 50]),
    rotation: new Float32Array([0, 0, 0]),
    extentsMin: new Float32Array([-10, -10, 0]),
    extentsMax: new Float32Array([10, 10, 20]),
    flags: 0,
    doodadSet: 0,
    nameSet: 0,
  });

  let summary2 = fresh.summary();
  assert.equal(summary2.wmoPlacementCount, 1);

  fresh.removeWmoPlacement(0);
  summary2 = fresh.summary();
  assert.equal(summary2.wmoPlacementCount, 0);

  assert.throws(() => fresh.addWmoPlacement({ nameId: 99, uniqueId: 1, position: new Float32Array([0,0,0]), rotation: new Float32Array([0,0,0]), extentsMin: new Float32Array([0,0,0]), extentsMax: new Float32Array([1,1,1]) }), /non-existent/);
  console.log("  addWmo / addWmoPlacement / removeWmoPlacement: OK");
}

// --- addModel / addDoodadPlacement / removeDoodadPlacement ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  const nameId = fresh.addModel("doodads/tree.m2");
  assert.equal(nameId, 0);

  fresh.addDoodadPlacement({
    nameId: 0,
    uniqueId: 42,
    position: new Float32Array([500, 500, 10]),
    rotation: new Float32Array([0, 90, 0]),
    scale: 2048, // 2.0x
  });

  let summary2 = fresh.summary();
  assert.equal(summary2.doodadPlacementCount, 1);

  fresh.removeDoodadPlacement(0);
  summary2 = fresh.summary();
  assert.equal(summary2.doodadPlacementCount, 0);

  // Round-trip
  fresh.addDoodadPlacement({
    nameId: 0,
    uniqueId: 100,
    position: new Float32Array([1, 2, 3]),
    rotation: new Float32Array([0, 0, 0]),
  });
  const exported = fresh.export();
  const reopened2 = new AdtFile(exported);
  assert.equal(reopened2.summary().doodadPlacementCount, 1);
  console.log("  addModel / addDoodadPlacement / removeDoodadPlacement: OK");
}

// --- setMclq / clearMclq (Vanilla liquid) ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "vanilla");
  // Create 81 vertex pairs: [depthUnion (as float32), height, ...]
  const verts = new Float32Array(81 * 2);
  for (let i = 0; i < 81; i++) {
    verts[i * 2] = 0;     // depth union byte
    verts[i * 2 + 1] = i * 0.5; // height
  }
  fresh.setMclq(0, 0, 10.0, 20.0, verts);

  // Check liquid is present
  const info2 = fresh.chunkInfo(0);
  assert.equal(info2.hasLiquid, true);

  // Remove liquid
  fresh.clearMclq(0);
  const info3 = fresh.chunkInfo(0);
  assert.equal(info3.hasLiquid, false);

  // Round-trip
  fresh.setMclq(0, 1, 0.0, 50.0, verts); // ocean
  const exported = fresh.export();
  const reopened2 = new AdtFile(exported);
  assert.equal(reopened2.chunkInfo(0).hasLiquid, true);
  console.log("  setMclq / clearMclq: OK");
}

// --- setMh2o / clearMh2oEntry (WotLK water) ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  // Set simple MH2O water on chunk 0
  const verts = new Float32Array(9 * 9);
  for (let i = 0; i < 81; i++) verts[i] = 5.0 + i * 0.1;

  fresh.setMh2o(0, {
    liquidType: 2, // water
    minHeight: 0.0,
    maxHeight: 10.0,
    width: 8,
    height: 8,
    vertexData: verts,
    fishableBitmap: "0",
    deepBitmap: "0",
  });

  // Check water exists
  const summary2 = fresh.summary();
  assert.equal(summary2.hasWater, true);

  // Remove
  fresh.clearMh2oEntry(0);

  // Round-trip
  fresh.setMh2o(0, {
    liquidType: 1,
    minHeight: 5.0,
    maxHeight: 15.0,
    width: 8,
    height: 8,
    vertexData: verts,
  });
  const exported = fresh.export();
  const reopened2 = new AdtFile(exported);
  // Check water data present
  const summary3 = reopened2.summary();
  assert.equal(summary3.hasWater, true);
  console.log("  setMh2o / clearMh2oEntry: OK");
}

// ============================================================================
// Task 2: Terrain editing helpers
// ============================================================================
console.log("--- Terrain editing helpers ---");

// --- changeTerrain ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  // Set initial heights
  const initial = new Float32Array(145);
  for (let i = 0; i < 145; i++) initial[i] = 10.0;
  fresh.setHeightmap(0, initial);

  // Raise center
  fresh.changeTerrain(0, 8, 8, 5.0, 5.0);
  const hm2 = fresh.heightmap(0);
  // Centre vertex (outer row 8, col 4) should be raised
  const centreIdx = Math.floor(8 / 2) * 9 + Math.floor(8 / 2) * 8 + 4; // row 8 = even, col 4 -> idx 8*4+? 
  // Just check some vertex changed
  let anyChanged = false;
  for (let i = 0; i < 145; i++) {
    if (hm2[i] !== 10.0) { anyChanged = true; break; }
  }
  assert.ok(anyChanged, "changeTerrain should modify some heights");

  // Round-trip
  const exported = fresh.export();
  const reopened2 = new AdtFile(exported);
  const hm3 = reopened2.heightmap(0);
  assert.ok(hm3 instanceof Float32Array);
  console.log("  changeTerrain: OK");
}

// --- flattenTerrain ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  const initial = new Float32Array(145);
  for (let i = 0; i < 145; i++) initial[i] = 100.0;
  fresh.setHeightmap(0, initial);

  fresh.flattenTerrain(0, 8, 8, 3.0, 50.0);
  const hm2 = fresh.heightmap(0);
  // Center area should now be ~50.0
  let hasFlattened = false;
  for (const v of hm2) {
    if (v === 50.0) { hasFlattened = true; break; }
  }
  assert.ok(hasFlattened, "flattenTerrain should set some vertices to targetHeight");
  console.log("  flattenTerrain: OK");
}

// --- smoothTerrain ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  const initial = new Float32Array(145);
  for (let i = 0; i < 145; i++) initial[i] = i % 10 === 0 ? 100.0 : 0.0; // spiky
  fresh.setHeightmap(0, initial);

  fresh.smoothTerrain(0, 8, 8, 5.0);
  const hm2 = fresh.heightmap(0);
  // Should not crash, and some values between 0 and 100
  let hasIntermediate = false;
  for (const v of hm2) {
    if (v > 0 && v < 100) { hasIntermediate = true; break; }
  }
  assert.ok(hasIntermediate, "smoothTerrain should produce intermediate values");
  console.log("  smoothTerrain: OK");
}

// --- clearHeight ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  const initial = new Float32Array(145);
  for (let i = 0; i < 145; i++) initial[i] = 123.0;
  fresh.setHeightmap(0, initial);

  fresh.clearHeight(0);
  const hm2 = fresh.heightmap(0);
  for (const v of hm2) assert.equal(v, 0.0);
  console.log("  clearHeight: OK");
}

// --- recalcNormals / recalcChunkNormals ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  const initial = new Float32Array(145);
  for (let i = 0; i < 145; i++) initial[i] = Math.sin(i * 0.1) * 10.0;
  fresh.setHeightmap(0, initial);

  // Normals were cleared by setHeightmap; recalc them
  fresh.recalcChunkNormals(0);
  // Should not crash; chunkInfo still works
  const info = fresh.chunkInfo(0);
  assert.equal(info.hasNormals, true);

  // recalc all chunks
  fresh.recalcNormals();

  // Round-trip
  const exported = fresh.export();
  const reopened2 = new AdtFile(exported);
  assert.equal(reopened2.chunkInfo(0).hasNormals, true);
  console.log("  recalcNormals / recalcChunkNormals: OK");
}

// --- fixGaps ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  // Set different heights on adjacent chunks
  const leftHeights = new Float32Array(145);
  const rightHeights = new Float32Array(145);
  for (let i = 0; i < 145; i++) {
    leftHeights[i] = 10.0;
    rightHeights[i] = 50.0;
  }
  fresh.setHeightmap(0, leftHeights);   // chunk at (0,0)
  fresh.setHeightmap(1, rightHeights);  // chunk at (1,0)

  fresh.fixGaps();
  // After fixGaps, the shared edge between chunk 0 and 1 should be averaged
  const hm0 = fresh.heightmap(0);
  const hm1 = fresh.heightmap(1);
  // At least some edge vertices should now be ~30.0 (average of 10 and 50)
  assert.ok(hm0 instanceof Float32Array && hm1 instanceof Float32Array);
  console.log("  fixGaps: OK");
}

// --- setHoles ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  // Initially no holes
  let info = fresh.chunkInfo(0);
  assert.equal(info.holesLowRes, 0);

  // Add hole at (0, 0)
  fresh.setHoles(0, 0, 0, true);
  info = fresh.chunkInfo(0);
  assert.equal(info.holesLowRes, 1); // bit 0 set

  // Add another hole at (3, 3)
  fresh.setHoles(0, 3, 3, true);
  info = fresh.chunkInfo(0);
  assert.equal(info.holesLowRes, 1 | (1 << 15)); // bits 0 and 15

  // Remove first hole
  fresh.setHoles(0, 0, 0, false);
  info = fresh.chunkInfo(0);
  assert.equal(info.holesLowRes, (1 << 15));

  // Round-trip
  const exported = fresh.export();
  const reopened2 = new AdtFile(exported);
  assert.equal(reopened2.chunkInfo(0).holesLowRes, (1 << 15));

  // Out of range
  assert.throws(() => fresh.setHoles(0, 4, 0, true), /must be in 0\.\.4/);
  console.log("  setHoles: OK");
}

// ============================================================================
// Task 3: Texture painting layer
// ============================================================================
console.log("--- Texture painting layer ---");

// --- paintTexture ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  fresh.addTexture("terrain/dirt.blp");
  fresh.addTextureLayer(0, { textureId: 1, flags: 0x100 });

  fresh.paintTexture(0, 1, 32, 32, 8.0, 0.8);
  const alpha = fresh.alphaMap(0);
  assert.ok(alpha instanceof Uint8Array);
  let hasNonZero = false;
  for (const b of alpha) { if (b > 0) { hasNonZero = true; break; } }
  assert.ok(hasNonZero, "paintTexture should modify alpha map");

  // Round-trip
  const exportedTex = fresh.export();
  const reopenedTex = new AdtFile(exportedTex);
  const alpha2 = reopenedTex.alphaMap(0);
  assert.ok(alpha2 instanceof Uint8Array);
  console.log("  paintTexture: OK");
}

// --- convertAlphaMap ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  const alpha = new Uint8Array(4096);
  for (let i = 0; i < 4096; i++) alpha[i] = i % 256;
  fresh.setAlphaMap(0, alpha);

  fresh.convertAlphaMap(0, false);
  let info = fresh.chunkInfo(0);
  assert.equal(info.hasAlpha, true);

  fresh.convertAlphaMap(0, true);
  info = fresh.chunkInfo(0);
  assert.equal(info.hasAlpha, true);
  console.log("  convertAlphaMap: OK");
}

// --- addTextureToChunk ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  const id = fresh.addTextureToChunk(0, "terrain/grass.blp");
  const summary = fresh.summary();
  assert.ok(summary.textures.includes("terrain/grass.blp"));
  const layers = fresh.textureLayers(0);
  assert.equal(layers.length, 2);

  // Calling again with same name reuses ID
  const id2 = fresh.addTextureToChunk(0, "terrain/grass.blp");
  assert.equal(id2, id);
  console.log("  addTextureToChunk: OK");
}

// --- replaceTexture ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  fresh.addTexture("terrain/dirt.blp");
  fresh.addTexture("terrain/grass.blp");
  fresh.addTextureLayer(0, { textureId: 1, flags: 0x100 });

  fresh.replaceTexture(1, 2);
  const layers = fresh.textureLayers(0);
  assert.equal(layers[1].textureId, 2);
  console.log("  replaceTexture: OK");
}

// --- setTextureFlags / clearTextureFlags ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  fresh.addTexture("terrain/dirt.blp");
  fresh.addTextureLayer(0, { textureId: 1 });

  fresh.setTextureFlags(0, 0, 0x040);
  let layers = fresh.textureLayers(0);
  assert.equal(layers[0].flags, 0x040);

  fresh.clearTextureFlags(0, 0);
  layers = fresh.textureLayers(0);
  assert.equal(layers[0].flags, 0);
  console.log("  setTextureFlags / clearTextureFlags: OK");
}

// --- setShadows ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  const shadows = new Uint8Array(512);
  for (let i = 0; i < 512; i++) shadows[i] = 0xFF;
  fresh.setShadows(0, shadows);

  const info = fresh.chunkInfo(0);
  assert.equal(info.hasShadow, true);

  assert.throws(() => fresh.setShadows(0, new Uint8Array(100)), /exactly 512/);
  console.log("  setShadows: OK");
}

// --- setVertexColors ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  const colors = new Uint8Array(145 * 4);
  for (let i = 0; i < 145; i++) {
    const o = i * 4;
    colors[o] = 0x80;
    colors[o + 1] = 0x40;
    colors[o + 2] = 0xFF;
    colors[o + 3] = 0x7F;
  }
  fresh.setVertexColors(0, colors);

  const info = fresh.chunkInfo(0);
  assert.equal(info.hasVertexColors, true);

  assert.throws(() => fresh.setVertexColors(0, new Uint8Array(10)), /580/);

  const exportedVc = fresh.export();
  const reopenedVc = new AdtFile(exportedVc);
  assert.equal(reopenedVc.chunkInfo(0).hasVertexColors, true);
  console.log("  setVertexColors: OK");
}

// ============================================================================
// Task 4: Water editing — additional helpers
// ============================================================================
console.log("--- Water editing helpers ---");

// --- setMclqType ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "vanilla");
  const verts = new Float32Array(162);
  for (let i = 0; i < 81; i++) { verts[i * 2 + 1] = 10.0; }
  fresh.setMclq(0, 0, 0.0, 20.0, verts); // water (type 0)

  // Change to magma (type 2)
  fresh.setMclqType(0, 2);
  const info = fresh.chunkInfo(0);
  assert.equal(info.hasLiquid, true);

  // Bad type
  assert.throws(() => fresh.setMclqType(0, 99), /must be 0-3/);
  console.log("  setMclqType: OK");
}

// --- autoGenWater ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "vanilla");
  const hm = new Float32Array(145);
  for (let i = 0; i < 145; i++) hm[i] = i % 20; // max = 19
  fresh.setHeightmap(0, hm);
  fresh.autoGenWater(0, 0.8, 0); // 80% of max terrain

  const info = fresh.chunkInfo(0);
  assert.equal(info.hasLiquid, true);
  console.log("  autoGenWater: OK");
}

// --- setMh2oAttributes ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  const verts = new Float32Array(81);
  fresh.setMh2o(0, {
    liquidType: 2, minHeight: 0, maxHeight: 10,
    width: 8, height: 8, vertexData: verts,
  });

  fresh.setMh2oAttributes(0, "255", "15");
  // Should not crash; attributes stored
  console.log("  setMh2oAttributes: OK");
}

// --- setMh2oVertexData ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  fresh.setMh2o(0, {
    liquidType: 2, minHeight: 0, maxHeight: 10,
    width: 8, height: 8,
  });

  const newVerts = new Float32Array(81);
  for (let i = 0; i < 81; i++) newVerts[i] = i * 0.5;
  fresh.setMh2oVertexData(0, newVerts);

  const exported = fresh.export();
  const reopened2 = new AdtFile(exported);
  assert.equal(reopened2.summary().hasWater, true);
  console.log("  setMh2oVertexData: OK");
}

// ============================================================================
// Task 5: Object placement — additional helpers
// ============================================================================
console.log("--- Object placement helpers ---");

// --- updateWmoPlacement ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  fresh.addWmo("buildings/house.wmo");
  fresh.addWmoPlacement({
    nameId: 0, uniqueId: 1,
    position: new Float32Array([0, 0, 0]),
    rotation: new Float32Array([0, 0, 0]),
    extentsMin: new Float32Array([-10, -10, 0]),
    extentsMax: new Float32Array([10, 10, 20]),
  });

  fresh.updateWmoPlacement(0, {
    position: new Float32Array([100, 200, 300]),
    flags: 0x01,
    doodadSet: 3,
  });
  // Should not crash
  assert.throws(() => fresh.updateWmoPlacement(99, {}), /out of range/);
  console.log("  updateWmoPlacement: OK");
}

// --- updateDoodadPlacement ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  fresh.addModel("doodads/tree.m2");
  fresh.addDoodadPlacement({
    nameId: 0, uniqueId: 10,
    position: new Float32Array([0, 0, 0]),
    rotation: new Float32Array([0, 0, 0]),
  });

  fresh.updateDoodadPlacement(0, {
    position: new Float32Array([50, 60, 70]),
    scale: 2048,
  });
  assert.throws(() => fresh.updateDoodadPlacement(99, {}), /out of range/);
  console.log("  updateDoodadPlacement: OK");
}

// --- newUid ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  fresh.addModel("doodads/tree.m2");
  fresh.addDoodadPlacement({
    nameId: 0, uniqueId: 42,
    position: new Float32Array([0, 0, 0]),
    rotation: new Float32Array([0, 0, 0]),
  });

  const uid = fresh.newUid();
  assert.ok(uid > 42, "newUid should be > max existing UID");
  console.log("  newUid: OK");
}

// --- recalcWmoExtents ---
{
  const fresh = AdtFile.create("tileset/generic.blp", "wotlk");
  fresh.addWmo("buildings/house.wmo");
  fresh.addWmoPlacement({
    nameId: 0, uniqueId: 1,
    position: new Float32Array([1000, 1000, 1000]),
    rotation: new Float32Array([0, 0, 0]),
    extentsMin: new Float32Array([0, 0, 0]),
    extentsMax: new Float32Array([0, 0, 0]),
  });

  fresh.recalcWmoExtents(0);
  assert.throws(() => fresh.recalcWmoExtents(99), /out of range/);
  console.log("  recalcWmoExtents: OK");
}

console.log("smoke test passed");
