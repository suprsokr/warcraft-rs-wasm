//! Web (wasm-bindgen) bindings for the `wow-mpq` MPQ archive library.
//!
//! Provides full read/write access to MPQ archives from JavaScript:
//! archives are passed in and out as `Uint8Array`s, and all parsing and
//! building happens in memory — no filesystem required.
//!
//! ```js
//! import init, { MpqArchive } from "wow-mpq-web";
//! await init();
//!
//! const archive = new MpqArchive(new Uint8Array(await file.arrayBuffer()));
//! console.log(archive.list());
//! const data = archive.readFile("readme.txt");
//!
//! archive.addFile("hello.txt", new TextEncoder().encode("hi"));
//! const modified = archive.export(); // Uint8Array with the new archive
//! ```

use std::collections::HashSet;
use std::io::Cursor;

use wasm_bindgen::prelude::*;
use wow_mpq::{Archive, ArchiveBuilder, header::FormatVersion};
use wow_web_common::{to_js, to_uint8_array};

/// A file entry as returned by [`MpqArchive::list`].
#[derive(serde::Serialize)]
struct ListedFile {
    name: String,
    /// Uncompressed size in bytes.
    size: u64,
    /// Compressed size in bytes (equals `size` for uncompressed files).
    compressed_size: u64,
    /// Raw MPQ block flags.
    flags: u32,
    compressed: bool,
    encrypted: bool,
    single_unit: bool,
    exists: bool,
}

/// MPQ reserves parenthesized names for internal special files.
fn is_special_file(name: &str) -> bool {
    matches!(name, "(listfile)" | "(attributes)" | "(signature)")
}

fn format_version(n: u8) -> Result<FormatVersion, JsError> {
    match n {
        1 => Ok(FormatVersion::V1),
        2 => Ok(FormatVersion::V2),
        3 => Ok(FormatVersion::V3),
        4 => Ok(FormatVersion::V4),
        _ => Err(JsError::new("version must be 1, 2, 3 or 4")),
    }
}

/// An MPQ archive opened in memory, with read access and staged writes.
///
/// Write operations ([`addFile`](MpqArchive::add_file),
/// [`removeFile`](MpqArchive::remove_file)) are staged and only materialize
/// when [`export()`](MpqArchive::export) rebuilds the archive.
#[wasm_bindgen]
pub struct MpqArchive {
    archive: Archive,
    /// Files staged for addition/replacement (name -> data).
    added: Vec<(String, Vec<u8>)>,
    /// Names staged for removal.
    removed: HashSet<String>,
}

#[wasm_bindgen]
impl MpqArchive {
    /// Open an existing MPQ archive from its raw bytes.
    #[wasm_bindgen(constructor)]
    pub fn open(data: &[u8]) -> Result<MpqArchive, JsError> {
        let archive = Archive::open_reader(Cursor::new(data.to_vec()))
            .map_err(|e| JsError::new(&format!("failed to open MPQ archive: {e}")))?;
        Ok(MpqArchive {
            archive,
            added: Vec::new(),
            removed: HashSet::new(),
        })
    }

    /// Create a new, empty MPQ archive.
    ///
    /// `version` is the MPQ format version (1–4, default 1).
    #[wasm_bindgen(js_name = create)]
    pub fn create(version: Option<u8>) -> Result<MpqArchive, JsError> {
        let version = format_version(version.unwrap_or(1))?;
        let mut out = Cursor::new(Vec::new());
        ArchiveBuilder::new()
            .version(version)
            .build_to_writer(&mut out)
            .map_err(|e| JsError::new(&format!("failed to create archive: {e}")))?;
        MpqArchive::open(out.get_ref())
    }

    /// List the files contained in the archive.
    ///
    /// Returns an array of
    /// `{ name, size, compressed_size, flags, compressed, encrypted, single_unit, exists }`.
    #[wasm_bindgen(js_name = list)]
    pub fn list(&mut self) -> Result<JsValue, JsError> {
        let entries = self
            .archive
            .list()
            .map_err(|e| JsError::new(&format!("failed to list archive: {e}")))?;
        let files: Vec<ListedFile> = entries
            .iter()
            .map(|e| ListedFile {
                name: e.name.clone(),
                size: e.size,
                compressed_size: e.compressed_size,
                flags: e.flags,
                compressed: e.is_compressed(),
                encrypted: e.is_encrypted(),
                single_unit: e.is_single_unit(),
                exists: e.exists(),
            })
            .collect();
        to_js(&files)
    }

    /// Read and decompress a file, returning its bytes.
    #[wasm_bindgen(js_name = readFile)]
    pub fn read_file(&mut self, name: &str) -> Result<js_sys::Uint8Array, JsError> {
        // Staged additions shadow the archive contents.
        if let Some((_, data)) = self.added.iter().rev().find(|(n, _)| n == name) {
            return Ok(js_sys::Uint8Array::from(data.as_slice()));
        }
        let data = self
            .archive
            .read_file(name)
            .map_err(|e| JsError::new(&format!("failed to read '{name}': {e}")))?;
        Ok(to_uint8_array(&data))
    }

    /// Stage a file for addition (or replacement) in the archive.
    /// Applied on [`export()`](MpqArchive::export).
    #[wasm_bindgen(js_name = addFile)]
    pub fn add_file(&mut self, name: &str, data: &[u8]) {
        self.removed.remove(name);
        self.added.push((name.to_string(), data.to_vec()));
    }

    /// Stage a file for removal from the archive.
    /// Applied on [`export()`](MpqArchive::export).
    #[wasm_bindgen(js_name = removeFile)]
    pub fn remove_file(&mut self, name: &str) {
        self.added.retain(|(n, _)| n != name);
        self.removed.insert(name.to_string());
    }

    /// Rebuild the archive with all staged changes applied and return its
    /// bytes. The internal state is swapped to the rebuilt archive, so you
    /// can keep editing afterwards.
    #[wasm_bindgen(js_name = export)]
    pub fn export(&mut self) -> Result<js_sys::Uint8Array, JsError> {
        let version = self.archive.header().format_version;

        let mut builder = ArchiveBuilder::new().version(version);

        // Carry over existing files that were not removed or replaced.
        let entries = self
            .archive
            .list()
            .map_err(|e| JsError::new(&format!("failed to list archive: {e}")))?;
        for entry in &entries {
            // Skip MPQ special files: the builder regenerates (listfile) and
            // (attributes) itself, and signatures cannot be preserved.
            if is_special_file(&entry.name)
                || self.removed.contains(&entry.name)
                || self.added.iter().any(|(n, _)| n == &entry.name)
            {
                continue;
            }
            let data = self
                .archive
                .read_file(&entry.name)
                .map_err(|e| JsError::new(&format!("failed to read '{}': {e}", entry.name)))?;
            builder = builder.add_file_data(data, &entry.name);
        }

        // Add staged files.
        for (name, data) in &self.added {
            builder = builder.add_file_data(data.clone(), name);
        }

        let mut out = Cursor::new(Vec::new());
        builder
            .build_to_writer(&mut out)
            .map_err(|e| JsError::new(&format!("failed to build archive: {e}")))?;

        // Swap internal state to the rebuilt archive.
        self.archive = Archive::open_reader(Cursor::new(out.get_ref().clone()))
            .map_err(|e| JsError::new(&format!("failed to reopen rebuilt archive: {e}")))?;
        self.added.clear();
        self.removed.clear();

        Ok(to_uint8_array(out.get_ref()))
    }

    /// Number of files currently visible (archive contents plus staged
    /// additions, minus staged removals).
    #[wasm_bindgen(js_name = fileCount)]
    pub fn file_count(&mut self) -> Result<usize, JsError> {
        let entries = self
            .archive
            .list()
            .map_err(|e| JsError::new(&format!("failed to list archive: {e}")))?;
        let kept = entries
            .iter()
            .filter(|e| {
                !self.removed.contains(&e.name) && !self.added.iter().any(|(n, _)| n == &e.name)
            })
            .count();
        Ok(kept + self.added.len())
    }
}
