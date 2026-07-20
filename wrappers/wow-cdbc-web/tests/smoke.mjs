import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import assert from "node:assert";

const pkgDir = join(dirname(fileURLToPath(import.meta.url)), "..", "pkg");
const {
  default: init,
  DbcFile,
} = await import(join(pkgDir, "wow_cdbc_web.js"));
await init({ module_or_path: readFileSync(join(pkgDir, "wow_cdbc_web_bg.wasm")) });

async function test() {
  // Build a minimal WDBC file:
  // Header: WDBC + record_count(2) + field_count(3) + record_size(12) + string_block_size(19)
  // = 20 bytes header
  // Records: 2 records × 3 fields × 4 bytes = 24 bytes
  // String block: "First\0Second\0Extra\0" = 19 bytes
  const header = new Uint8Array([
    0x57, 0x44, 0x42, 0x43, // "WDBC"
    0x02, 0x00, 0x00, 0x00, // record_count = 2
    0x03, 0x00, 0x00, 0x00, // field_count = 3
    0x0C, 0x00, 0x00, 0x00, // record_size = 12
    0x13, 0x00, 0x00, 0x00, // string_block_size = 19
  ]);
  const records = new Uint8Array([
    // Record 1: ID=1, Name offset=0, Value=100
    0x01, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
    0x64, 0x00, 0x00, 0x00,
    // Record 2: ID=2, Name offset=6, Value=200
    0x02, 0x00, 0x00, 0x00,
    0x06, 0x00, 0x00, 0x00,
    0xC8, 0x00, 0x00, 0x00,
  ]);
  const strings = new TextEncoder().encode("First\0Second\0Extra\0");
  const data = new Uint8Array([...header, ...records, ...strings]);

  // Test parse + summary
  const dbc = new DbcFile(data);
  const summary = dbc.summary();
  assert.strictEqual(summary.magic, "WDBC");
  assert.strictEqual(summary.recordCount, 2);
  assert.strictEqual(summary.fieldCount, 3);
  assert.strictEqual(summary.recordSize, 12);
  assert.strictEqual(summary.stringBlockSize, 19);

  // Test raw records
  const raw = dbc.records();
  assert.strictEqual(raw.length, 2);
  assert.deepStrictEqual(raw[0], [1, 0, 100]);
  assert.deepStrictEqual(raw[1], [2, 6, 200]);

  // Test schema-driven records
  const schema = [
    { name: "ID", type: "uint32" },
    { name: "Name", type: "string" },
    { name: "Value", type: "uint32" },
  ];
  const typed = dbc.recordsWithSchema(schema);
  assert.strictEqual(typed.length, 2);
  assert.strictEqual(typed[0].ID, 1);
  assert.strictEqual(typed[0].Name, "First");
  assert.strictEqual(typed[0].Value, 100);
  assert.strictEqual(typed[1].ID, 2);
  assert.strictEqual(typed[1].Name, "Second");
  assert.strictEqual(typed[1].Value, 200);

  // Test strings
  const strs = dbc.strings();
  assert.ok(strs.includes("First"));
  assert.ok(strs.includes("Second"));

  // Test round-trip export (semantic equality, not byte-for-byte)
  const exported = dbc.export();
  assert.ok(exported instanceof Uint8Array);
  assert.ok(exported.length > 0);

  // Re-parse the exported bytes and verify semantic equality
  const dbc2 = new DbcFile(exported);
  const summary2 = dbc2.summary();
  assert.strictEqual(summary2.recordCount, 2);
  assert.strictEqual(summary2.fieldCount, 3);

  const typed2 = dbc2.recordsWithSchema(schema);
  assert.strictEqual(typed2.length, 2);
  assert.strictEqual(typed2[0].ID, 1);
  assert.strictEqual(typed2[0].Name, "First");
  assert.strictEqual(typed2[0].Value, 100);
  assert.strictEqual(typed2[1].ID, 2);
  assert.strictEqual(typed2[1].Name, "Second");
  assert.strictEqual(typed2[1].Value, 200);

  console.log("wow-cdbc-web smoke test passed");
}

test().catch((e) => {
  console.error(e);
  process.exit(1);
});
