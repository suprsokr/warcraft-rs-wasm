//! Web (wasm-bindgen) bindings for the `wow-wdt` WDT (World Data Table)
//! library.
//!
//! WDT files describe which tiles of a map exist, plus optional global WMO
//! placement. Everything happens in memory: bytes in (`Uint8Array`),
//! plain data out.
//!
//! ```js
//! import init, { WdtFile } from "wow-wdt-web";
//! await init();
//!
//! const wdt = new WdtFile(new Uint8Array(await file.arrayBuffer()), "3.3.5a");
//! console.log(wdt.summary());
//! console.log(wdt.tiles());          // [{x, y, hasAdt, areaId, flags}, ...]
//!
//! wdt.setTile(32, 32, true, 0);      // mark a tile as existing
//! const bytes = wdt.export();        // Uint8Array with the new WDT
//! ```

use std::io::Cursor;

use serde::Serialize;
use wasm_bindgen::prelude::*;
use wow_wdt::chunks::MwmoChunk;
use wow_wdt::version::WowVersion;
use wow_wdt::{WdtFile, WdtReader, WdtWriter};
use wow_web_common::{to_js, to_uint8_array};

/// Accepts an expansion name ("classic", "tbc", "wotlk", "cata", "mop",
/// "wod", "legion", "bfa", "shadowlands", "dragonflight") or a version
/// string ("1.12.1", "3.3.5a", …).
fn parse_version(s: &str) -> Result<WowVersion, JsError> {
    let lower = s.to_ascii_lowercase();
    let named = match lower.as_str() {
        "classic" | "vanilla" => Some(WowVersion::Classic),
        "tbc" => Some(WowVersion::TBC),
        "wotlk" | "wrath" => Some(WowVersion::WotLK),
        "cata" | "cataclysm" => Some(WowVersion::Cataclysm),
        "mop" => Some(WowVersion::MoP),
        "wod" => Some(WowVersion::WoD),
        "legion" => Some(WowVersion::Legion),
        "bfa" => Some(WowVersion::BfA),
        "shadowlands" | "sl" => Some(WowVersion::Shadowlands),
        "dragonflight" | "df" => Some(WowVersion::Dragonflight),
        _ => None,
    };
    match named {
        Some(v) => Ok(v),
        None => WowVersion::from_string(s).map_err(|e| {
            JsError::new(&format!(
                "invalid version \"{s}\" (use an expansion name like \"wotlk\" or a version string like \"3.3.5a\"): {e}"
            ))
        }),
    }
}

fn version_name(version: WowVersion) -> &'static str {
    match version {
        WowVersion::Classic => "classic",
        WowVersion::TBC => "tbc",
        WowVersion::WotLK => "wotlk",
        WowVersion::Cataclysm => "cataclysm",
        WowVersion::MoP => "mop",
        WowVersion::WoD => "wod",
        WowVersion::Legion => "legion",
        WowVersion::BfA => "bfa",
        WowVersion::Shadowlands => "shadowlands",
        WowVersion::Dragonflight => "dragonflight",
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Tile {
    x: usize,
    y: usize,
    has_adt: bool,
    area_id: u32,
    flags: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WmoPlacement {
    id: u32,
    unique_id: u32,
    position: [f32; 3],
    rotation: [f32; 3],
    flags: u16,
    doodad_set: u16,
    name_set: u16,
    scale: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WdtSummary {
    version: String,
    is_wmo_only: bool,
    mphd_flags: u32,
    tile_count: usize,
    has_maid: bool,
    wmo_filenames: Vec<String>,
    wmo_placements: Vec<WmoPlacement>,
    validation_warnings: Vec<String>,
}

/// A WDT file opened in memory, with read access and mutation.
///
/// Mutations apply to the in-memory representation immediately;
/// [`export()`](Wdt::export) serializes back to WDT bytes.
#[wasm_bindgen(js_name = WdtFile)]
pub struct Wdt {
    file: WdtFile,
}

#[wasm_bindgen(js_class = WdtFile)]
impl Wdt {
    /// Open an existing WDT file from its raw bytes.
    ///
    /// `version` is an expansion name ("wotlk", …) or version string
    /// ("3.3.5a", …) — the WDT format is version-dependent and the file
    /// itself does not store which WoW version it belongs to.
    #[wasm_bindgen(constructor)]
    pub fn open(data: &[u8], version: &str) -> Result<Wdt, JsError> {
        let version = parse_version(version)?;
        let mut reader = WdtReader::new(Cursor::new(data.to_vec()), version);
        let file = reader
            .read()
            .map_err(|e| JsError::new(&format!("failed to parse WDT: {e}")))?;
        Ok(Wdt { file })
    }

    /// Create a new, empty WDT file for the given version.
    #[wasm_bindgen(js_name = create)]
    pub fn create(version: &str) -> Result<Wdt, JsError> {
        Ok(Wdt {
            file: WdtFile::new(parse_version(version)?),
        })
    }

    /// A summary of the file: version, flags, tile count, WMO filenames and
    /// placements, and version-aware validation warnings.
    #[wasm_bindgen(js_name = summary)]
    pub fn summary(&self) -> Result<JsValue, JsError> {
        let file = &self.file;
        let summary = WdtSummary {
            version: version_name(file.version()).to_string(),
            is_wmo_only: file.is_wmo_only(),
            mphd_flags: file.mphd.flags.bits(),
            tile_count: file.count_existing_tiles(),
            has_maid: file.maid.is_some(),
            wmo_filenames: file
                .mwmo
                .as_ref()
                .map(|m| m.filenames.clone())
                .unwrap_or_default(),
            wmo_placements: file
                .modf
                .as_ref()
                .map(|m| {
                    m.entries
                        .iter()
                        .map(|e| WmoPlacement {
                            id: e.id,
                            unique_id: e.unique_id,
                            position: e.position,
                            rotation: e.rotation,
                            flags: e.flags,
                            doodad_set: e.doodad_set,
                            name_set: e.name_set,
                            scale: e.scale,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            validation_warnings: file.validate(),
        };
        to_js(&summary)
    }

    /// List all existing tiles as `[{x, y, hasAdt, areaId, flags}, ...]`.
    #[wasm_bindgen(js_name = tiles)]
    pub fn tiles(&self) -> Result<JsValue, JsError> {
        let mut tiles = Vec::new();
        for y in 0..64 {
            for x in 0..64 {
                if let Some(info) = self.file.get_tile(x, y)
                    && info.has_adt
                {
                    tiles.push(Tile {
                        x: info.x,
                        y: info.y,
                        has_adt: info.has_adt,
                        area_id: info.area_id,
                        flags: info.flags,
                    });
                }
            }
        }
        to_js(&tiles)
    }

    /// Get information about a single tile, or `undefined` if out of range.
    #[wasm_bindgen(js_name = getTile)]
    pub fn get_tile(&self, x: u32, y: u32) -> Result<JsValue, JsError> {
        match self.file.get_tile(x as usize, y as usize) {
            Some(info) => to_js(&Tile {
                x: info.x,
                y: info.y,
                has_adt: info.has_adt,
                area_id: info.area_id,
                flags: info.flags,
            }),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// Set whether a tile exists (has ADT data), optionally setting its
    /// area id. Coordinates must be in `0..64`.
    #[wasm_bindgen(js_name = setTile)]
    pub fn set_tile(
        &mut self,
        x: u32,
        y: u32,
        has_adt: bool,
        area_id: Option<u32>,
    ) -> Result<(), JsError> {
        let entry = self
            .file
            .main
            .get_mut(x as usize, y as usize)
            .ok_or_else(|| JsError::new("tile coordinates must be in 0..64"))?;
        entry.set_has_adt(has_adt);
        if let Some(area_id) = area_id {
            entry.area_id = area_id;
        }
        Ok(())
    }

    /// Add a global WMO filename (for WMO-only maps).
    #[wasm_bindgen(js_name = addWmoFilename)]
    pub fn add_wmo_filename(&mut self, filename: &str) {
        self.file
            .mwmo
            .get_or_insert_with(MwmoChunk::new)
            .add_filename(filename.to_string());
    }

    /// Serialize the file back to WDT bytes.
    #[wasm_bindgen(js_name = export)]
    pub fn export(&self) -> Result<js_sys::Uint8Array, JsError> {
        let mut out = Vec::new();
        WdtWriter::new(&mut out)
            .write(&self.file)
            .map_err(|e| JsError::new(&format!("failed to write WDT: {e}")))?;
        Ok(to_uint8_array(&out))
    }
}
