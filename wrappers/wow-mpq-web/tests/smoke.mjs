// Smoke test for the wow-mpq-web wasm bindings.
// Run after building pkg/ (see package-web job in ci.yml):
//   node wrappers/wow-mpq-web/tests/smoke.mjs
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import assert from "node:assert/strict";

const pkg = join(dirname(fileURLToPath(import.meta.url)), "..", "pkg");
const { default: init, MpqArchive, MpqStreamingArchive } = await import(join(pkg, "wow_mpq_web.js"));
await init({ module_or_path: readFileSync(join(pkg, "wow_mpq_web_bg.wasm")) });

// --- create / add / export / reopen ---
const a = MpqArchive.create(1);
a.addFile("hello.txt", new TextEncoder().encode("hello world"));
a.addFile("dir/nested.txt", new TextEncoder().encode("nested content"));
const bytes = a.export();
assert.ok(bytes.length > 0, "export produced bytes");

const b = new MpqArchive(bytes);
const files = b.list().map((f) => f.name);
assert.ok(files.includes("hello.txt"), "hello.txt present");
assert.ok(files.includes("dir\\nested.txt"), "nested file present (MPQ path separator)");
assert.equal(
  new TextDecoder().decode(b.readFile("hello.txt")),
  "hello world",
  "file contents round-trip",
);

// --- staged replacement ---
b.addFile("hello.txt", new TextEncoder().encode("replaced"));
const b2 = new MpqArchive(b.export());
assert.equal(new TextDecoder().decode(b2.readFile("hello.txt")), "replaced");

// --- staged removal ---
b2.removeFile("hello.txt");
const c = new MpqArchive(b2.export());
assert.ok(!c.list().some((f) => f.name === "hello.txt"), "hello.txt removed");

// --- error handling ---
assert.throws(() => new MpqArchive(new Uint8Array([1, 2, 3])), /MPQ|failed/);
assert.throws(() => c.readFile("does-not-exist.txt"));

// --- streaming archive from a File (browser-style read callback) ---
// b holds the archive where hello.txt was replaced; use it before the
// removal test mutates it away.
const streamingBytes = b.export();
const streamingFile = new File([streamingBytes], "streaming.mpq");
const readRange = (_file, offset, length) => streamingBytes.slice(offset, offset + length);
const s = new MpqStreamingArchive(streamingFile, readRange);
const streamingFiles = s.list().map((f) => f.name);
assert.ok(streamingFiles.includes("dir\\nested.txt"), "streaming list sees nested file");
assert.equal(
  new TextDecoder().decode(s.readFile("hello.txt")),
  "replaced",
  "streaming readFile works",
);
const listfile = s.listfile();
assert.ok(Array.isArray(listfile), "(listfile) is parsed as an array");
assert.ok(listfile.includes("hello.txt"), "(listfile) contains hello.txt");
assert.ok(listfile.includes("dir\\nested.txt"), "(listfile) contains nested file");
s.free();

console.log("smoke test passed");
