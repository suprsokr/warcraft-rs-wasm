# wow-cdbc-web

WebAssembly bindings for [`wow-cdbc`](../../file-formats/database/wow-cdbc) —
parse and write World of Warcraft DBC (client database) files from
JavaScript / TypeScript.

## API

### `DbcFile`

```js
import init, { DbcFile } from "wow-cdbc-web";
await init();

const dbc = new DbcFile(bytes);

// Header summary
console.log(dbc.summary());
// { magic: "WDBC", recordCount: 123, fieldCount: 5, recordSize: 20, stringBlockSize: 256 }

// Raw records (no schema — all fields as UInt32)
const records = dbc.records();
// [[1, 0, 100], [2, 6, 200], ...]

// Typed records with a schema
const schema = [
  { name: "ID", type: "uint32" },
  { name: "Name", type: "string" },
  { name: "Value", type: "int32" },
];
const typed = dbc.recordsWithSchema(schema);
// [{ ID: 1, Name: "First", Value: 100 }, { ID: 2, Name: "Second", Value: 200 }, ...]

// All strings from the string block
const strings = dbc.strings();
// ["First", "Second", "Extra"]

// Round-trip export
const bytes = dbc.export();
```

### Field types

| Type | Size | JS output type |
|------|------|----------------|
| `int32` | 4 bytes | number |
| `uint32` | 4 bytes | number |
| `float32` | 4 bytes | number |
| `string` | 4 bytes (offset) | string (resolved) |
| `bool` | 4 bytes | boolean |
| `uint8` | 1 byte | number |
| `int8` | 1 byte | number |
| `uint16` | 2 bytes | number |
| `int16` | 2 bytes | number |

## Building

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli

cargo build --release -p wow-cdbc-web --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir wrappers/wow-cdbc-web/pkg \
  target/wasm32-unknown-unknown/release/wow_cdbc_web.wasm
```
