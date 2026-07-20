//! Web (wasm-bindgen) bindings for the `wow-adt` ADT terrain tile library.
//!
//! ADT tiles are the 16×16 chunk terrain tiles of a WoW map. Everything
//! happens in memory: bytes in (`Uint8Array`), plain data out.
//!
//! Terrain tiles are large, so the API is deliberately two-level:
//! [`summary()`](Adt::summary) returns cheap metadata (versions, name
//! lists, counts), while heavy per-chunk data is fetched on demand via
//! [`heightmap()`](Adt::heightmap) / [`chunkInfo()`](Adt::chunk_info) /
//! [`alphaMap()`](Adt::alpha_map).
//!
//! ```js
//! import init, { AdtFile } from "wow-adt-web";
//! await init();
//!
//! // Monolithic (pre-Cataclysm) tile, or the root file of a split set:
//! const adt = new AdtFile(new Uint8Array(await file.arrayBuffer()));
//! console.log(adt.summary());
//! const heights = adt.heightmap(0);    // Float32Array(145) or undefined
//!
//! // Cataclysm+ split set: the resolver is called lazily for each
//! // expected companion file (rootName_tex0.adt, _obj0.adt, _lod.adt).
//! const adt2 = AdtFile.loadSplit("Azeroth_30_30.adt", rootBytes, (name) =>
//!   mpq.hasFile(name) ? mpq.readFile(name) : undefined,
//! );
//! ```

use std::io::Cursor;

use serde::Serialize;
use wasm_bindgen::prelude::*;
use wow_adt::adt_set::AdtSet;
use wow_adt::builder::AdtBuilder;
use wow_adt::version::AdtVersion;
use wow_adt::{ParsedAdt, parse_adt};
use wow_web_common::{to_js, to_uint8_array};

/// Accepts an expansion-ish name: "vanilla", "vanilla-late", "tbc",
/// "wotlk", "cata", "mop".
fn parse_version(s: &str) -> Result<AdtVersion, JsError> {
    match s.to_ascii_lowercase().as_str() {
        "vanilla" | "classic" | "vanilla-early" => Ok(AdtVersion::VanillaEarly),
        "vanilla-late" | "vanilla1.9" => Ok(AdtVersion::VanillaLate),
        "tbc" => Ok(AdtVersion::TBC),
        "wotlk" | "wrath" => Ok(AdtVersion::WotLK),
        "cata" | "cataclysm" => Ok(AdtVersion::Cataclysm),
        "mop" => Ok(AdtVersion::MoP),
        _ => Err(JsError::new(&format!(
            "invalid version \"{s}\" (use \"vanilla\", \"vanilla-late\", \"tbc\", \"wotlk\", \"cata\", or \"mop\")"
        ))),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdtSummary {
    version: String,
    terrain_chunk_count: usize,
    textures: Vec<String>,
    models: Vec<String>,
    wmos: Vec<String>,
    doodad_placement_count: usize,
    wmo_placement_count: usize,
    has_water: bool,
    has_flight_bounds: bool,
    has_texture_flags: bool,
    has_blend_meshes: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChunkInfo {
    index_x: u32,
    index_y: u32,
    /// World position [X, Y, Z]. Vertex heights are relative to position[1].
    position: [f32; 3],
    area_id: u32,
    flags: u32,
    holes_low_res: u16,
    /// 64-bit hole map, present on MoP 5.3+ chunks (serialized as string
    /// because JS numbers can't hold u64 exactly).
    holes_high_res: Option<String>,
    layer_count: usize,
    has_heights: bool,
    has_normals: bool,
    has_alpha: bool,
    has_shadow: bool,
    has_vertex_colors: bool,
    has_liquid: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TextureLayer {
    texture_id: u32,
    flags: u32,
    offset_in_mcal: u32,
    effect_id: u32,
}

fn parse_root(data: &[u8]) -> Result<wow_adt::api::RootAdt, JsError> {
    let mut cursor = Cursor::new(data);
    match parse_adt(&mut cursor).map_err(|e| JsError::new(&format!("failed to parse ADT: {e}")))? {
        ParsedAdt::Root(root) => Ok(*root),
        _ => Err(JsError::new(
            "expected a root ADT file (got a _tex0/_obj0/_lod split file); \
             use AdtFile.loadSplit(rootName, rootBytes, resolver) for split sets",
        )),
    }
}

/// An ADT terrain tile parsed in memory.
///
/// Heavy per-chunk data (heightmaps, alpha maps) is exposed through
/// on-demand accessors; [`summary()`](Adt::summary) is cheap.
/// [`export()`](Adt::export) serializes back to a monolithic root ADT.
#[wasm_bindgen(js_name = AdtFile)]
pub struct Adt {
    root: wow_adt::api::RootAdt,
}

#[wasm_bindgen(js_class = AdtFile)]
impl Adt {
    /// Open an existing ADT from raw bytes (monolithic file, or the root
    /// file of a split set — companion files are *not* merged in that
    /// case; use `loadSplit` for that).
    #[wasm_bindgen(constructor)]
    pub fn open(data: &[u8]) -> Result<Adt, JsError> {
        Ok(Adt {
            root: parse_root(data)?,
        })
    }

    /// Load a Cataclysm+ split ADT set and merge it into a single tile.
    ///
    /// `resolver` is a JS function `(name: string) => Uint8Array | null | undefined`.
    /// It is called for each expected companion file derived from
    /// `rootName` (`<stem>_tex0.adt`, `<stem>_obj0.adt`, `<stem>_lod.adt`);
    /// returning `undefined`/`null` means the file doesn't exist. This
    /// composes naturally with `wow-mpq-web`:
    ///
    /// ```js
    /// const adt = AdtFile.loadSplit("World/Maps/Azeroth/Azeroth_30_30.adt",
    ///   rootBytes, (name) => archive.hasFile(name) ? archive.readFile(name) : undefined);
    /// ```
    #[wasm_bindgen(js_name = loadSplit)]
    pub fn load_split(
        root_name: &str,
        root_bytes: &[u8],
        resolver: js_sys::Function,
    ) -> Result<Adt, JsError> {
        let resolve = |name: &str| -> Option<Vec<u8>> {
            let value = resolver
                .call1(&JsValue::NULL, &JsValue::from_str(name))
                .ok()?;
            if value.is_null() || value.is_undefined() {
                return None;
            }
            js_sys::Uint8Array::new(&value).to_vec().into()
        };
        let set = AdtSet::from_named_bytes(root_name, root_bytes, resolve)
            .map_err(|e| JsError::new(&format!("failed to parse ADT split set: {e}")))?;
        let root = set
            .merge()
            .map_err(|e| JsError::new(&format!("failed to merge ADT split set: {e}")))?;
        Ok(Adt { root })
    }

    /// Create a new minimal ADT tile from scratch.
    ///
    /// `textureName` is the base terrain texture (forward slashes, `.blp`
    /// extension). The serializer auto-generates 256 empty terrain chunks.
    /// `version` is optional ("vanilla" default, or "vanilla-late",
    /// "tbc", "wotlk", "cata", "mop").
    #[wasm_bindgen(js_name = create)]
    pub fn create(texture_name: &str, version: Option<String>) -> Result<Adt, JsError> {
        let mut builder = AdtBuilder::new();
        if let Some(v) = version {
            builder = builder.with_version(parse_version(&v)?);
        }
        let built = builder
            .add_texture(texture_name)
            .build()
            .map_err(|e| JsError::new(&format!("failed to build ADT: {e}")))?;
        let bytes = built
            .to_bytes()
            .map_err(|e| JsError::new(&format!("failed to serialize ADT: {e}")))?;
        Ok(Adt {
            root: parse_root(&bytes)?,
        })
    }

    /// Cheap metadata: version, texture/model/WMO name lists, counts and
    /// feature flags.
    #[wasm_bindgen(js_name = summary)]
    pub fn summary(&self) -> Result<JsValue, JsError> {
        let root = &self.root;
        to_js(&AdtSummary {
            version: root.version.to_string(),
            terrain_chunk_count: root.mcnk_chunks.len(),
            textures: root.textures.clone(),
            models: root.models.clone(),
            wmos: root.wmos.clone(),
            doodad_placement_count: root.doodad_placements.len(),
            wmo_placement_count: root.wmo_placements.len(),
            has_water: root.water_data.is_some(),
            has_flight_bounds: root.flight_bounds.is_some(),
            has_texture_flags: root.texture_flags.is_some(),
            has_blend_meshes: root.blend_mesh_headers.is_some(),
        })
    }

    /// Number of MCNK terrain chunks (usually 256).
    #[wasm_bindgen(js_name = chunkCount)]
    pub fn chunk_count(&self) -> usize {
        self.root.mcnk_chunks.len()
    }

    /// Metadata for terrain chunk `index` (0..chunkCount): position, area
    /// id, holes, layer count and which subchunks are present.
    #[wasm_bindgen(js_name = chunkInfo)]
    pub fn chunk_info(&self, index: usize) -> Result<JsValue, JsError> {
        let chunk = self
            .root
            .mcnk_chunks
            .get(index)
            .ok_or_else(|| JsError::new("chunk index out of range"))?;
        let h = &chunk.header;
        to_js(&ChunkInfo {
            index_x: h.index_x,
            index_y: h.index_y,
            position: h.world_position(),
            area_id: h.area_id,
            flags: h.flags.value,
            holes_low_res: h.holes_low_res,
            holes_high_res: h.holes_high_res().map(|v| v.to_string()),
            layer_count: chunk.layers.as_ref().map_or(0, |l| l.layers.len()),
            has_heights: chunk.heights.is_some(),
            has_normals: chunk.normals.is_some(),
            has_alpha: chunk.alpha.is_some(),
            has_shadow: chunk.shadow.is_some(),
            has_vertex_colors: chunk.vertex_colors.is_some(),
            has_liquid: chunk.liquid.is_some(),
        })
    }

    /// Heightmap for terrain chunk `index`: 145 f32 values (9×9 outer +
    /// 8×8 inner grid, row-major 17 rows of 9/8), relative to
    /// `chunkInfo(index).position[1]`. Returns `undefined` if the chunk
    /// has no MCVT data.
    #[wasm_bindgen(js_name = heightmap)]
    pub fn heightmap(&self, index: usize) -> Result<JsValue, JsError> {
        let chunk = self
            .root
            .mcnk_chunks
            .get(index)
            .ok_or_else(|| JsError::new("chunk index out of range"))?;
        match &chunk.heights {
            Some(h) => Ok(js_sys::Float32Array::from(h.heights.as_slice()).into()),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// Texture layers for terrain chunk `index`:
    /// `[{textureId, flags, offsetInMcal, effectId}, ...]`. `textureId`
    /// indexes into `summary().textures`.
    #[wasm_bindgen(js_name = textureLayers)]
    pub fn texture_layers(&self, index: usize) -> Result<JsValue, JsError> {
        let chunk = self
            .root
            .mcnk_chunks
            .get(index)
            .ok_or_else(|| JsError::new("chunk index out of range"))?;
        let layers: Vec<TextureLayer> = chunk
            .layers
            .as_ref()
            .map(|l| {
                l.layers
                    .iter()
                    .map(|layer| TextureLayer {
                        texture_id: layer.texture_id,
                        flags: layer.flags.value,
                        offset_in_mcal: layer.offset_in_mcal,
                        effect_id: layer.effect_id,
                    })
                    .collect()
            })
            .unwrap_or_default();
        to_js(&layers)
    }

    /// Raw MCAL alpha-map data for terrain chunk `index` (all layers,
    /// concatenated; use `textureLayers(index)[n].offsetInMcal` to slice
    /// per layer). Returns `undefined` if the chunk has no alpha data.
    #[wasm_bindgen(js_name = alphaMap)]
    pub fn alpha_map(&self, index: usize) -> Result<JsValue, JsError> {
        let chunk = self
            .root
            .mcnk_chunks
            .get(index)
            .ok_or_else(|| JsError::new("chunk index out of range"))?;
        match &chunk.alpha {
            Some(a) => Ok(js_sys::Uint8Array::from(a.data.as_slice()).into()),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// Serialize back to a monolithic root ADT file.
    #[wasm_bindgen(js_name = export)]
    pub fn export(&self) -> Result<js_sys::Uint8Array, JsError> {
        let built = AdtBuilder::from_parsed(self.root.clone())
            .build()
            .map_err(|e| JsError::new(&format!("failed to build ADT: {e}")))?;
        let bytes = built
            .to_bytes()
            .map_err(|e| JsError::new(&format!("failed to serialize ADT: {e}")))?;
        Ok(to_uint8_array(&bytes))
    }
}
