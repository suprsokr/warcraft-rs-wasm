//! Web (wasm-bindgen) bindings for the `wow-wdl` WDL (low-resolution
//! terrain) library.
//!
//! WDL files store 17x17 low-res heightmaps per map tile for distant
//! terrain rendering. Everything happens in memory: bytes in
//! (`Uint8Array`), plain data out.
//!
//! ```js
//! import init, { WdlFile } from "wow-wdl-web";
//! await init();
//!
//! const wdl = new WdlFile(new Uint8Array(await file.arrayBuffer()));
//! console.log(wdl.summary());
//! const hm = wdl.heightmap(32, 32);  // { outer: Int16Array(289), inner: Int16Array(256) }
//!
//! wdl.setHeightmap(32, 32, hm.outer, hm.inner);
//! const bytes = wdl.export();        // Uint8Array with the new WDL
//! ```

use std::io::Cursor;

use serde::Serialize;
use wasm_bindgen::prelude::*;
use wow_wdl::parser::WdlParser;
use wow_wdl::types::{HeightMapTile, HolesData, WdlFile};
use wow_wdl::version::WdlVersion;
use wow_web_common::{to_js, to_uint8_array};

/// Accepts an expansion name: "vanilla", "wotlk", "cataclysm", "mop",
/// "wod", "legion", "bfa", "shadowlands", "dragonflight" or "latest".
fn parse_version(s: &str) -> Result<WdlVersion, JsError> {
    match s.to_ascii_lowercase().as_str() {
        "vanilla" | "classic" | "tbc" => Ok(WdlVersion::Vanilla),
        "wotlk" | "wrath" => Ok(WdlVersion::Wotlk),
        "cata" | "cataclysm" => Ok(WdlVersion::Cataclysm),
        "mop" => Ok(WdlVersion::Mop),
        "wod" => Ok(WdlVersion::Wod),
        "legion" => Ok(WdlVersion::Legion),
        "bfa" => Ok(WdlVersion::Bfa),
        "shadowlands" | "sl" => Ok(WdlVersion::Shadowlands),
        "dragonflight" | "df" => Ok(WdlVersion::Dragonflight),
        "latest" => Ok(WdlVersion::Latest),
        other => Err(JsError::new(&format!(
            "invalid version \"{other}\" (use an expansion name like \"wotlk\" or \"latest\")"
        ))),
    }
}

fn version_name(version: WdlVersion) -> &'static str {
    match version {
        WdlVersion::Vanilla => "vanilla",
        WdlVersion::Wotlk => "wotlk",
        WdlVersion::Cataclysm => "cataclysm",
        WdlVersion::Mop => "mop",
        WdlVersion::Wod => "wod",
        WdlVersion::Legion => "legion",
        WdlVersion::Bfa => "bfa",
        WdlVersion::Shadowlands => "shadowlands",
        WdlVersion::Dragonflight => "dragonflight",
        WdlVersion::Latest => "latest",
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TileCoord {
    x: u32,
    y: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WdlSummary {
    version: String,
    version_number: u32,
    tile_count: usize,
    holes_count: usize,
    wmo_filenames: Vec<String>,
}

fn check_coords(x: u32, y: u32) -> Result<(), JsError> {
    if x >= 64 || y >= 64 {
        return Err(JsError::new("tile coordinates must be in 0..64"));
    }
    Ok(())
}

/// A WDL file opened in memory, with read access and mutation.
///
/// Mutations apply to the in-memory representation immediately;
/// [`export()`](Wdl::export) serializes back to WDL bytes (offsets are
/// recomputed on write).
#[wasm_bindgen(js_name = WdlFile)]
pub struct Wdl {
    file: WdlFile,
}

#[wasm_bindgen(js_class = WdlFile)]
impl Wdl {
    /// Open an existing WDL file from its raw bytes.
    ///
    /// All WDL format versions share the same MVER version number, so the
    /// format version cannot be detected from content. Pass an optional
    /// `version` hint (expansion name like "wotlk"; default "latest") —
    /// it controls which optional chunks (MWMO/ML**) are expected.
    #[wasm_bindgen(constructor)]
    pub fn open(data: &[u8], version: Option<String>) -> Result<Wdl, JsError> {
        let version = parse_version(version.as_deref().unwrap_or("latest"))?;
        let file = WdlParser::with_version(version)
            .parse(&mut Cursor::new(data.to_vec()))
            .map_err(|e| JsError::new(&format!("failed to parse WDL: {e}")))?;
        Ok(Wdl { file })
    }

    /// Create a new, empty WDL file for the given version
    /// (expansion name, e.g. "wotlk"; default in the format sense is
    /// "latest").
    #[wasm_bindgen(js_name = create)]
    pub fn create(version: &str) -> Result<Wdl, JsError> {
        let mut file = WdlFile::new();
        let version = parse_version(version)?;
        file.version = version;
        file.version_number = version.version_number();
        Ok(Wdl { file })
    }

    /// A summary of the file: version, tile count and WMO filenames.
    #[wasm_bindgen(js_name = summary)]
    pub fn summary(&self) -> Result<JsValue, JsError> {
        let summary = WdlSummary {
            version: version_name(self.file.version).to_string(),
            version_number: self.file.version_number,
            tile_count: self.file.heightmap_tiles.len(),
            holes_count: self.file.holes_data.len(),
            wmo_filenames: self.file.wmo_filenames.clone(),
        };
        to_js(&summary)
    }

    /// List the coordinates of all tiles that have heightmap data, as
    /// `[{x, y}, ...]`.
    #[wasm_bindgen(js_name = tiles)]
    pub fn tiles(&self) -> Result<JsValue, JsError> {
        let mut tiles: Vec<TileCoord> = self
            .file
            .heightmap_tiles
            .keys()
            .map(|&(x, y)| TileCoord { x, y })
            .collect();
        tiles.sort_by_key(|t| (t.y, t.x));
        to_js(&tiles)
    }

    /// Get the heightmap of a tile as
    /// `{ outer: Int16Array(289), inner: Int16Array(256) }`
    /// (17x17 corner values + 16x16 center values), or `undefined` if the
    /// tile has no data.
    #[wasm_bindgen(js_name = heightmap)]
    pub fn heightmap(&self, x: u32, y: u32) -> Result<JsValue, JsError> {
        check_coords(x, y)?;
        let Some(tile) = self.file.heightmap_tiles.get(&(x, y)) else {
            return Ok(JsValue::UNDEFINED);
        };
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(
            &obj,
            &JsValue::from_str("outer"),
            &js_sys::Int16Array::from(tile.outer_values.as_slice()),
        )
        .unwrap();
        js_sys::Reflect::set(
            &obj,
            &JsValue::from_str("inner"),
            &js_sys::Int16Array::from(tile.inner_values.as_slice()),
        )
        .unwrap();
        Ok(obj.into())
    }

    /// Set the heightmap of a tile. `outer` must have 289 (17x17) values,
    /// `inner` 256 (16x16) values.
    #[wasm_bindgen(js_name = setHeightmap)]
    pub fn set_heightmap(
        &mut self,
        x: u32,
        y: u32,
        outer: &[i16],
        inner: &[i16],
    ) -> Result<(), JsError> {
        check_coords(x, y)?;
        if outer.len() != 289 {
            return Err(JsError::new(&format!(
                "outer must have 289 (17x17) values, got {}",
                outer.len()
            )));
        }
        if inner.len() != 256 {
            return Err(JsError::new(&format!(
                "inner must have 256 (16x16) values, got {}",
                inner.len()
            )));
        }
        self.file.heightmap_tiles.insert(
            (x, y),
            HeightMapTile {
                outer_values: outer.to_vec(),
                inner_values: inner.to_vec(),
            },
        );
        Ok(())
    }

    /// Remove a tile's heightmap (and holes) data.
    #[wasm_bindgen(js_name = removeTile)]
    pub fn remove_tile(&mut self, x: u32, y: u32) -> Result<(), JsError> {
        check_coords(x, y)?;
        self.file.heightmap_tiles.remove(&(x, y));
        self.file.holes_data.remove(&(x, y));
        Ok(())
    }

    /// Get the hole bitmasks of a tile as `Uint16Array(16)` (one bitmask
    /// per row, bits 0–15 = chunks 0–15), or `undefined` if the tile has
    /// no holes data.
    #[wasm_bindgen(js_name = holes)]
    pub fn holes(&self, x: u32, y: u32) -> Result<JsValue, JsError> {
        check_coords(x, y)?;
        match self.file.holes_data.get(&(x, y)) {
            Some(holes) => Ok(js_sys::Uint16Array::from(holes.hole_masks.as_slice()).into()),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// Set the hole bitmasks of a tile. `masks` must have 16 values.
    #[wasm_bindgen(js_name = setHoles)]
    pub fn set_holes(&mut self, x: u32, y: u32, masks: &[u16]) -> Result<(), JsError> {
        check_coords(x, y)?;
        if masks.len() != 16 {
            return Err(JsError::new(&format!(
                "masks must have 16 values, got {}",
                masks.len()
            )));
        }
        let mut hole_masks = [0u16; 16];
        hole_masks.copy_from_slice(masks);
        self.file
            .holes_data
            .insert((x, y), HolesData { hole_masks });
        Ok(())
    }

    /// Serialize the file back to WDL bytes.
    #[wasm_bindgen(js_name = export)]
    pub fn export(&self) -> Result<js_sys::Uint8Array, JsError> {
        let mut out = Cursor::new(Vec::new());
        WdlParser::new()
            .write(&mut out, &self.file)
            .map_err(|e| JsError::new(&format!("failed to write WDL: {e}")))?;
        Ok(to_uint8_array(out.get_ref()))
    }
}
