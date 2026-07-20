// Smoke test for the wow-wmo-web wasm bindings.
// Run after building pkg/ (see package-web job in ci.yml):
//   node wrappers/wow-wmo-web/tests/smoke.mjs
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import assert from "node:assert/strict";

const pkg = join(dirname(fileURLToPath(import.meta.url)), "..", "pkg");
const { default: init, WmoFile } = await import(join(pkg, "wow_wmo_web.js"));
await init({ module_or_path: readFileSync(join(pkg, "wow_wmo_web_bg.wasm")) });

// --- create / export / reopen round-trip ---
const wmo = WmoFile.create("wotlk");
wmo.addTexture("tileset/dungeon.blp");
const bytes = wmo.exportRoot();
assert.ok(bytes.length > 0, "export produced bytes");

const reopened = new WmoFile(bytes);
const summary = reopened.summary();
// Version 17 is detected as classic by default (all pre-WoD variants share v17)
assert.ok(["classic", "wotlk", "cata", "mop"].includes(summary.version));
assert.deepEqual(summary.textures, ["tileset/dungeon.blp"]);
assert.equal(summary.groupCount, 0);
assert.equal(summary.materialCount, 0);
assert.equal(summary.loadedGroupCount, 0);
assert.equal(summary.skybox, undefined);

// --- group count ---
assert.equal(reopened.groupCount(), 0);

// --- error: group not loaded / out of range ---
assert.throws(() => reopened.groupVertices(0), /not loaded/);
assert.throws(() => new WmoFile(new Uint8Array([1, 2, 3])), /failed to parse/);

// --- create with other versions ---
const cata = WmoFile.create("cata");
assert.equal(cata.summary().version, "cataclysm");

// --- loadWithGroups resolver is called for each group ---
const calls = [];
const withGroups = WmoFile.loadWithGroups("test.wmo", bytes, (name) => {
  calls.push(name);
  return undefined;
});
assert.equal(withGroups.summary().version, "classic");
// root has zero groups, so resolver should not have been called
assert.deepEqual(calls, []);

// --- invalid version ---
assert.throws(() => WmoFile.create("not-a-version"), /invalid version/);

console.log("smoke test passed");
