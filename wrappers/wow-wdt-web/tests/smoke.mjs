// Smoke test for the wow-wdt-web wasm bindings.
// Run after building pkg/ (see package-web job in ci.yml):
//   node wrappers/wow-wdt-web/tests/smoke.mjs
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import assert from "node:assert/strict";

const pkg = join(dirname(fileURLToPath(import.meta.url)), "..", "pkg");
const { default: init, WdtFile } = await import(join(pkg, "wow_wdt_web.js"));
await init({ module_or_path: readFileSync(join(pkg, "wow_wdt_web_bg.wasm")) });

// --- create / mutate / export / reopen round-trip ---
const wdt = WdtFile.create("wotlk");
wdt.setTile(32, 32, true, 0);
wdt.setTile(33, 32, true, 42);
wdt.setTile(34, 32, true); // areaId optional
const bytes = wdt.export();
assert.ok(bytes.length > 0, "export produced bytes");

// version string form also accepted
const reopened = new WdtFile(bytes, "3.3.5a");
const summary = reopened.summary();
// Note: wow-wdt re-detects the version from content on read; a terrain map
// without an MWMO chunk is detected as Cataclysm+ regardless of the
// version hint passed to the constructor.
assert.equal(summary.version, "cataclysm");
assert.equal(summary.tileCount, 3);
assert.equal(summary.isWmoOnly, false);
assert.equal(summary.hasMaid, false);
assert.deepEqual(summary.wmoFilenames, []);

const tiles = reopened.tiles();
assert.equal(tiles.length, 3);
const t = tiles.find((t) => t.x === 33 && t.y === 32);
assert.ok(t, "tile 33,32 present");
assert.equal(t.hasAdt, true);
assert.equal(t.areaId, 42);

// --- getTile / setTile(false) ---
const g = reopened.getTile(32, 32);
assert.equal(g.hasAdt, true);
reopened.setTile(32, 32, false);
assert.equal(reopened.getTile(32, 32).hasAdt, false);
const bytes2 = reopened.export();
assert.equal(new WdtFile(bytes2, "wotlk").tiles().length, 2);

// --- WMO-only map ---
const wmo = WdtFile.create("classic");
wmo.addWmoFilename("World\\wmo\\dungeon.wmo");
const wmoBytes = wmo.export();
const wmoSummary = new WdtFile(wmoBytes, "1.12.1").summary();
assert.deepEqual(wmoSummary.wmoFilenames, ["World\\wmo\\dungeon.wmo"]);

// --- error handling ---
assert.throws(() => new WdtFile(new Uint8Array([1, 2, 3]), "wotlk"), /failed to parse WDT/);
assert.throws(() => new WdtFile(bytes, "not-a-version"), /invalid version/);
assert.throws(() => reopened.setTile(64, 0, true), /0\.\.64/);

console.log("smoke test passed");
