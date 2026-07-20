// Smoke test for the wow-wdl-web wasm bindings.
// Run after building pkg/ (see package-web job in ci.yml):
//   node wrappers/wow-wdl-web/tests/smoke.mjs
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import assert from "node:assert/strict";

const pkg = join(dirname(fileURLToPath(import.meta.url)), "..", "pkg");
const { default: init, WdlFile } = await import(join(pkg, "wow_wdl_web.js"));
await init({ module_or_path: readFileSync(join(pkg, "wow_wdl_web_bg.wasm")) });

// --- create / mutate / export / reopen round-trip ---
const wdl = WdlFile.create("wotlk");
const outer = new Int16Array(289);
const inner = new Int16Array(256);
for (let i = 0; i < 289; i++) outer[i] = i - 100;
for (let i = 0; i < 256; i++) inner[i] = 500 + i;
wdl.setHeightmap(32, 32, outer, inner);
wdl.setHeightmap(10, 20, outer, inner);
const masks = new Uint16Array(16);
masks[0] = 0b1010101010101010;
wdl.setHoles(32, 32, masks);
const bytes = wdl.export();
assert.ok(bytes.length > 0, "export produced bytes");

const reopened = new WdlFile(bytes, "wotlk");
const summary = reopened.summary();
assert.equal(summary.version, "wotlk");
assert.equal(summary.tileCount, 2);
assert.equal(summary.holesCount, 1);

const tiles = reopened.tiles();
assert.deepEqual(tiles, [{ x: 10, y: 20 }, { x: 32, y: 32 }]);

// --- heightmap / holes round-trip ---
const hm = reopened.heightmap(32, 32);
assert.ok(hm.outer instanceof Int16Array);
assert.deepEqual([...hm.outer], [...outer]);
assert.deepEqual([...hm.inner], [...inner]);
assert.equal(reopened.heightmap(0, 0), undefined);
const holes = reopened.holes(32, 32);
assert.ok(holes instanceof Uint16Array);
assert.equal(holes[0], 0b1010101010101010);
assert.equal(reopened.holes(10, 20), undefined);

// --- removeTile ---
reopened.removeTile(10, 20);
const bytes2 = reopened.export();
const again = new WdlFile(bytes2, "wotlk");
assert.equal(again.tiles().length, 1);
assert.equal(again.heightmap(10, 20), undefined);

// --- error handling ---
assert.throws(() => new WdlFile(new Uint8Array([1, 2, 3])), /failed to parse WDL/);
assert.throws(() => WdlFile.create("nope"), /invalid version/);
assert.throws(() => wdl.setHeightmap(64, 0, outer, inner), /0\.\.64/);
assert.throws(() => wdl.setHeightmap(0, 0, new Int16Array(3), inner), /289/);
assert.throws(() => wdl.setHeightmap(0, 0, outer, new Int16Array(3)), /256/);
assert.throws(() => wdl.setHoles(0, 0, new Uint16Array(3)), /16 values/);

console.log("smoke test passed");
