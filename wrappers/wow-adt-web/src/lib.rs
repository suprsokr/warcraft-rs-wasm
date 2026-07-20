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

// Re-export chunk types we need for construction
use wow_adt::chunks::mcnk::mclq::{LiquidType, LiquidVertex, MclqChunk};
use wow_adt::chunks::mcnk::{McalChunk, MclyChunk, MclyFlags, MclyLayer, McnkFlags, McvtChunk};
use wow_adt::chunks::mh2o::vertex::{HeightDepthVertex, VertexDataArray};
use wow_adt::chunks::mh2o::{Mh2oAttributes, Mh2oChunk, Mh2oEntry, Mh2oHeader, Mh2oInstance};
use wow_adt::chunks::{DoodadPlacement, WmoPlacement};

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
///
/// ## Mutating API (Task 1)
///
/// The `root` field is now accessible through typed setter methods that
/// mutate the ADT in memory. Changes are reflected in subsequent
/// [`export()`](Adt::export) calls. See the individual setter methods
/// for details.
#[wasm_bindgen(js_name = AdtFile)]
pub struct Adt {
    root: wow_adt::api::RootAdt,
}

#[wasm_bindgen(js_class = AdtFile)]
impl Adt {
    // ========================================================================
    // Constructors
    // ========================================================================

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

    // ========================================================================
    // Read-only accessors
    // ========================================================================

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
        let chunk = self.chunk_at(index)?;
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
        let chunk = self.chunk_at(index)?;
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
        let chunk = self.chunk_at(index)?;
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
        let chunk = self.chunk_at(index)?;
        match &chunk.alpha {
            Some(a) => Ok(js_sys::Uint8Array::from(a.data.as_slice()).into()),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    // ========================================================================
    // Mutating API — Heightmap & chunk info
    // ========================================================================

    /// Write the 145-value MCVT heightmap for chunk `index`.
    ///
    /// `heights` must be a `Float32Array` of length 145.
    #[wasm_bindgen(js_name = setHeightmap)]
    pub fn set_heightmap(&mut self, index: usize, heights: &[f32]) -> Result<(), JsError> {
        if heights.len() != 145 {
            return Err(JsError::new("heightmap must be exactly 145 floats"));
        }
        let chunk = self.chunk_at_mut(index)?;
        let mcvt = McvtChunk {
            heights: heights.to_vec(),
        };
        chunk.heights = Some(mcvt);
        // Mark normals dirty — caller should recalcNormals afterward
        chunk.normals = None;
        // Update header offset/size will be handled by the serializer on export
        Ok(())
    }

    /// Mutate MCNK header fields for chunk `index`.
    ///
    /// `changes` is a plain JS object with any of:
    /// `position` ([x,y,z]), `areaId`, `flags`, `holesLowRes`.
    #[wasm_bindgen(js_name = setChunkInfo)]
    pub fn set_chunk_info(&mut self, index: usize, changes: JsValue) -> Result<(), JsError> {
        let chunk = self.chunk_at_mut(index)?;

        if let Some(v) = get_opt_f32_array(&changes, "position", 3)? {
            // Store in file order: [Z, X, Y]
            chunk.header.position = [v[2], v[0], v[1]];
        }
        if let Some(v) = get_opt_u32(&changes, "areaId")? {
            chunk.header.area_id = v;
        }
        if let Some(v) = get_opt_u32(&changes, "flags")? {
            chunk.header.flags = McnkFlags { value: v };
        }
        if let Some(v) = get_opt_u32(&changes, "holesLowRes")? {
            chunk.header.holes_low_res = v as u16;
        }
        Ok(())
    }

    // ========================================================================
    // Mutating API — Textures
    // ========================================================================

    /// Add a texture name to MTEX. Returns the new texture ID (index).
    #[wasm_bindgen(js_name = addTexture)]
    pub fn add_texture(&mut self, name: String) -> Result<usize, JsError> {
        if name.is_empty() {
            return Err(JsError::new("texture name must not be empty"));
        }
        self.root.textures.push(name);
        Ok(self.root.textures.len() - 1)
    }

    /// Remove a texture from MTEX by ID. All texture IDs above `textureId`
    /// in MCLY layers are decremented. Removes any layers that reference
    /// the removed texture.
    #[wasm_bindgen(js_name = removeTexture)]
    pub fn remove_texture(&mut self, texture_id: usize) -> Result<(), JsError> {
        if texture_id >= self.root.textures.len() {
            return Err(JsError::new("texture ID out of range"));
        }
        self.root.textures.remove(texture_id);

        // Fix MCLY references in all chunks
        for chunk in &mut self.root.mcnk_chunks {
            if let Some(layers) = &mut chunk.layers {
                // Remove any layer referencing the deleted texture,
                // and decrement IDs > texture_id
                layers
                    .layers
                    .retain(|layer| (layer.texture_id as usize) != texture_id);
                for layer in &mut layers.layers {
                    if (layer.texture_id as usize) > texture_id {
                        layer.texture_id -= 1;
                    }
                }
            }
        }
        Ok(())
    }

    /// Add a texture layer (MCLY entry) to chunk `index`.
    ///
    /// `layer` is a JS object with: `textureId`, `flags`, `effectId`.
    /// `offsetInMcal` is computed automatically.
    #[wasm_bindgen(js_name = addTextureLayer)]
    pub fn add_texture_layer(&mut self, index: usize, layer: JsValue) -> Result<(), JsError> {
        let texture_id = get_u32(&layer, "textureId")?;
        let flags_val = get_opt_u32(&layer, "flags")?.unwrap_or(0x100); // default: use_alpha_map
        let effect_id = get_opt_u32(&layer, "effectId")?.unwrap_or(0);

        // Validate texture_id before mutating chunk
        if (texture_id as usize) >= self.root.textures.len() {
            return Err(JsError::new("textureId out of range"));
        }

        let mcly_layer = MclyLayer {
            texture_id,
            flags: MclyFlags { value: flags_val },
            offset_in_mcal: 0, // Will be computed on export
            effect_id,
        };

        let chunk = self.chunk_at_mut(index)?;
        match &mut chunk.layers {
            Some(layers) => {
                if layers.layers.len() >= 4 {
                    return Err(JsError::new("chunk already has max 4 layers"));
                }
                layers.layers.push(mcly_layer);
            }
            None => {
                chunk.layers = Some(MclyChunk {
                    layers: vec![mcly_layer],
                });
                chunk.header.n_layers = 1;
            }
        }
        chunk.header.n_layers = chunk.layers.as_ref().map_or(0, |l| l.layers.len() as u32);
        Ok(())
    }

    /// Remove a texture layer by index from chunk `index`.
    #[wasm_bindgen(js_name = removeTextureLayer)]
    pub fn remove_texture_layer(
        &mut self,
        index: usize,
        layer_index: usize,
    ) -> Result<(), JsError> {
        let chunk = self.chunk_at_mut(index)?;
        let layers = chunk
            .layers
            .as_mut()
            .ok_or_else(|| JsError::new("chunk has no texture layers"))?;
        if layer_index >= layers.layers.len() {
            return Err(JsError::new("layer index out of range"));
        }
        if layers.layers.len() <= 1 {
            return Err(JsError::new("cannot remove the last/base texture layer"));
        }
        layers.layers.remove(layer_index);
        chunk.header.n_layers = layers.layers.len() as u32;
        Ok(())
    }

    // ========================================================================
    // Mutating API — Alpha maps
    // ========================================================================

    /// Set raw MCAL data for chunk `index`.
    ///
    /// `data` is a `Uint8Array` containing the raw MCAL bytes for the
    /// entire chunk (all layers concatenated). The serializer will encode
    /// these on `export()`.
    #[wasm_bindgen(js_name = setAlphaMap)]
    pub fn set_alpha_map(&mut self, index: usize, data: &[u8]) -> Result<(), JsError> {
        let chunk = self.chunk_at_mut(index)?;
        chunk.alpha = Some(McalChunk::new(data.to_vec()));
        Ok(())
    }

    // ========================================================================
    // Mutating API — WMO / Models
    // ========================================================================

    /// Add a WMO filename to MWMO. Returns the new name ID (index).
    #[wasm_bindgen(js_name = addWmo)]
    pub fn add_wmo(&mut self, name: String) -> Result<usize, JsError> {
        if name.is_empty() {
            return Err(JsError::new("WMO name must not be empty"));
        }
        self.root.wmos.push(name);
        Ok(self.root.wmos.len() - 1)
    }

    /// Add a WMO placement (MODF entry).
    ///
    /// `placement` is a JS object with:
    /// `nameId`, `uniqueId`, `position` ([x,y,z]), `rotation` ([x,y,z]),
    /// `extentsMin` ([x,y,z]), `extentsMax` ([x,y,z]), `flags`, `doodadSet`,
    /// `nameSet`, `scale` (defaults: 1024=1.0).
    #[wasm_bindgen(js_name = addWmoPlacement)]
    pub fn add_wmo_placement(&mut self, placement: JsValue) -> Result<(), JsError> {
        let name_id = get_u32(&placement, "nameId")?;
        let unique_id = get_u32(&placement, "uniqueId")?;
        let position = get_f32_array(&placement, "position", 3)?;
        let rotation = get_f32_array(&placement, "rotation", 3)?;
        let extents_min = get_f32_array(&placement, "extentsMin", 3)?;
        let extents_max = get_f32_array(&placement, "extentsMax", 3)?;
        let flags = get_opt_u32(&placement, "flags")?.unwrap_or(0) as u16;
        let doodad_set = get_opt_u32(&placement, "doodadSet")?.unwrap_or(0) as u16;
        let name_set = get_opt_u32(&placement, "nameSet")?.unwrap_or(0) as u16;
        let scale = get_opt_u32(&placement, "scale")?.unwrap_or(1024) as u16;

        if (name_id as usize) >= self.root.wmos.len() {
            return Err(JsError::new("nameId references non-existent WMO"));
        }

        let wmo_placement = WmoPlacement {
            name_id,
            unique_id,
            position,
            rotation,
            extents_min,
            extents_max,
            flags,
            doodad_set,
            name_set,
            scale,
        };

        self.root.wmo_placements.push(wmo_placement);
        Ok(())
    }

    /// Remove a WMO placement by index.
    #[wasm_bindgen(js_name = removeWmoPlacement)]
    pub fn remove_wmo_placement(&mut self, index: usize) -> Result<(), JsError> {
        if index >= self.root.wmo_placements.len() {
            return Err(JsError::new("WMO placement index out of range"));
        }
        self.root.wmo_placements.remove(index);
        Ok(())
    }

    /// Add an M2 model filename to MMDX. Returns the new name ID (index).
    #[wasm_bindgen(js_name = addModel)]
    pub fn add_model(&mut self, name: String) -> Result<usize, JsError> {
        if name.is_empty() {
            return Err(JsError::new("model name must not be empty"));
        }
        self.root.models.push(name);
        Ok(self.root.models.len() - 1)
    }

    /// Add a doodad placement (MDDF entry).
    ///
    /// `placement` is a JS object with:
    /// `nameId`, `uniqueId`, `position` ([x,y,z]), `rotation` ([x,y,z]),
    /// `scale` (default: 1024=1.0), `flags`.
    #[wasm_bindgen(js_name = addDoodadPlacement)]
    pub fn add_doodad_placement(&mut self, placement: JsValue) -> Result<(), JsError> {
        let name_id = get_u32(&placement, "nameId")?;
        let unique_id = get_u32(&placement, "uniqueId")?;
        let position = get_f32_array(&placement, "position", 3)?;
        let rotation = get_f32_array(&placement, "rotation", 3)?;
        let scale = get_opt_u32(&placement, "scale")?.unwrap_or(1024) as u16;
        let flags = get_opt_u32(&placement, "flags")?.unwrap_or(0) as u16;

        if (name_id as usize) >= self.root.models.len() {
            return Err(JsError::new("nameId references non-existent model"));
        }

        let doodad_placement = DoodadPlacement {
            name_id,
            unique_id,
            position,
            rotation,
            scale,
            flags,
        };

        self.root.doodad_placements.push(doodad_placement);
        Ok(())
    }

    /// Remove a doodad placement by index.
    #[wasm_bindgen(js_name = removeDoodadPlacement)]
    pub fn remove_doodad_placement(&mut self, index: usize) -> Result<(), JsError> {
        if index >= self.root.doodad_placements.len() {
            return Err(JsError::new("doodad placement index out of range"));
        }
        self.root.doodad_placements.remove(index);
        Ok(())
    }

    // ========================================================================
    // Mutating API — Liquid (MCLQ legacy)
    // ========================================================================

    /// Set MCLQ legacy liquid for chunk `index` (Vanilla/TBC only).
    ///
    /// `config` is a JS object with:
    /// `liquidType` (0=water, 1=ocean, 2=magma, 3=slime),
    /// `minHeight`, `maxHeight`,
    /// `vertices` (Float32Array of 81*2=162 values: 81 heights packed as
    ///    [depth/union, height] pairs),
    /// `tileFlags` (Uint8Array of 64 values, optional).
    ///
    /// Uses the multi-layer `liquid_layers` field so multiple layers
    /// (e.g., water above lava) are supported.
    #[wasm_bindgen(js_name = setMclq)]
    pub fn set_mclq(
        &mut self,
        index: usize,
        liquid_type: u8,
        min_height: f32,
        max_height: f32,
        vertices: JsValue,
    ) -> Result<(), JsError> {
        let chunk = self.chunk_at_mut(index)?;

        let liquid_type = match liquid_type {
            0 => LiquidType::Water,
            1 => LiquidType::Ocean,
            2 => LiquidType::Magma,
            3 => LiquidType::Slime,
            _ => return Err(JsError::new("liquidType must be 0-3")),
        };

        // vertices: expects 81 * 2 floats = [depth_byte, height, ...] repeated 81 times
        // or 81 * 2 where first element is the union_data (stored as f32 but cast to 4 bytes)
        let vert_data = js_sys::Float32Array::new(&vertices).to_vec();
        if vert_data.len() != 81 * 2 {
            return Err(JsError::new(
                "vertices must be Float32Array of 162 elements (81 pairs of [depthUnion, height])",
            ));
        }

        let mut liquid_verts = Vec::with_capacity(81);
        for i in 0..81 {
            let raw = vert_data[i * 2];
            let height = vert_data[i * 2 + 1];
            // Encode raw as 4 bytes
            let union_data = raw.to_le_bytes();
            liquid_verts.push(LiquidVertex { union_data, height });
        }

        let mclq = MclqChunk {
            min_height,
            max_height,
            vertices: liquid_verts,
            tile_flags: [0u8; 64], // Default: all visible
            liquid_type,
        };

        // Store in multi-layer field; also set single-layer for backward compat
        chunk.liquid_layers = Some(vec![mclq.clone()]);
        chunk.liquid = Some(mclq);

        // Update MCNK flags to signal liquid type
        let flag_bit = match liquid_type {
            LiquidType::Water => 0x04,
            LiquidType::Ocean => 0x08,
            LiquidType::Magma => 0x10,
            LiquidType::Slime => 0x20,
        };
        // Clear existing liquid bits, set new one
        chunk.header.flags.value &= !(0x04 | 0x08 | 0x10 | 0x20);
        chunk.header.flags.value |= flag_bit;

        Ok(())
    }

    /// Remove MCLQ from chunk `index`.
    #[wasm_bindgen(js_name = clearMclq)]
    pub fn clear_mclq(&mut self, index: usize) -> Result<(), JsError> {
        let chunk = self.chunk_at_mut(index)?;
        chunk.liquid = None;
        chunk.liquid_layers = None;
        chunk.header.flags.value &= !(0x04 | 0x08 | 0x10 | 0x20);
        Ok(())
    }

    // ========================================================================
    // Mutating API — MH2O (WotLK+ water)
    // ========================================================================

    /// Set MH2O entry for chunk `index` (WotLK+ only).
    ///
    /// `config` is a JS object with:
    /// `liquidType` (u32), `minHeight`, `maxHeight`,
    /// `width` (u8, 0-8), `height` (u8, 0-8),
    /// `existsBitmap` (u64 as string, optional),
    /// `vertexData` (Float32Array or Uint8Array, depends on LVF),
    /// `fishableBitmap` (u64 as string, optional),
    /// `deepBitmap` (u64 as string, optional).
    #[wasm_bindgen(js_name = setMh2o)]
    pub fn set_mh2o(&mut self, index: usize, config: JsValue) -> Result<(), JsError> {
        let liquid_type = get_u32(&config, "liquidType")? as u16;
        let min_height = get_f32(&config, "minHeight")?;
        let max_height = get_f32(&config, "maxHeight")?;
        let width = get_opt_u32(&config, "width")?.unwrap_or(8) as u8;
        let height = get_opt_u32(&config, "height")?.unwrap_or(8) as u8;

        // Read vertex data (optional Float32Array)
        let vertex_data =
            if let Ok(v) = js_sys::Reflect::get(&config, &JsValue::from_str("vertexData")) {
                if v.is_undefined() || v.is_null() {
                    None
                } else {
                    let floats = js_sys::Float32Array::new(&v).to_vec();
                    let render_count = (width as usize + 1) * (height as usize + 1);
                    if floats.len() >= render_count {
                        // Build sparse 9x9 array (81 elements)
                        let mut arr: Box<[Option<HeightDepthVertex>; 81]> =
                            Box::new([const { None }; 81]);
                        for z in 0..=(height as usize) {
                            for x in 0..=(width as usize) {
                                let idx = z * (width as usize + 1) + x;
                                let grid_idx = (z) * 9 + x;
                                if idx < floats.len() && grid_idx < 81 {
                                    arr[grid_idx] = Some(HeightDepthVertex {
                                        height: floats[idx],
                                        depth: 0,
                                    });
                                }
                            }
                        }
                        Some(VertexDataArray::HeightDepth(arr))
                    } else {
                        None
                    }
                }
            } else {
                None
            };

        // Ensure MH2O chunk exists
        if self.root.water_data.is_none() {
            self.root.water_data = Some(Mh2oChunk::new());
        }
        let water = self.root.water_data.as_mut().unwrap();

        // Ensure entry exists
        if index >= water.entries.len() {
            return Err(JsError::new(
                "chunk index out of range for MH2O (should be 0-255)",
            ));
        }

        let entry = &mut water.entries[index];
        entry.header = Mh2oHeader {
            offset_instances: 0,
            layer_count: 1,
            offset_attributes: 0,
        };
        entry.instances = vec![Mh2oInstance {
            liquid_type,
            liquid_object_or_lvf: 0, // LVF 0
            min_height_level: min_height,
            max_height_level: max_height,
            x_offset: 0,
            y_offset: 0,
            width,
            height,
            offset_exists_bitmap: 0,
            offset_vertex_data: 0,
        }];
        entry.vertex_data = vec![vertex_data];

        // Attribute bitmaps (optional)
        if let Some(fish) = get_opt_u64_str(&config, "fishableBitmap")? {
            if let Some(deep) = get_opt_u64_str(&config, "deepBitmap")? {
                entry.attributes = Some(Mh2oAttributes {
                    fishable: fish,
                    deep,
                });
            } else {
                entry.attributes = Some(Mh2oAttributes {
                    fishable: fish,
                    deep: 0,
                });
            }
        }

        Ok(())
    }

    /// Remove MH2O entry for chunk `index`.
    #[wasm_bindgen(js_name = clearMh2oEntry)]
    pub fn clear_mh2o_entry(&mut self, index: usize) -> Result<(), JsError> {
        if let Some(water) = &mut self.root.water_data {
            if index < water.entries.len() {
                water.entries[index] = Mh2oEntry::default();
            }
        }
        Ok(())
    }

    // ========================================================================
    // Terrain editing helpers (Task 2)
    // ========================================================================

    /// Recompute MCNR normals for all chunks that have height data.
    /// Call this after any heightmap edits.
    #[wasm_bindgen(js_name = recalcNormals)]
    pub fn recalc_normals(&mut self) -> Result<(), JsError> {
        for i in 0..self.root.mcnk_chunks.len() {
            self.recalc_chunk_normals(i)?;
        }
        Ok(())
    }

    /// Recompute MCNR normals for a single chunk from its MCVT heights.
    #[wasm_bindgen(js_name = recalcChunkNormals)]
    pub fn recalc_chunk_normals(&mut self, index: usize) -> Result<(), JsError> {
        let chunk = self.chunk_at_mut(index)?;
        let heights = match &chunk.heights {
            Some(h) => &h.heights,
            None => return Ok(()),
        };
        if heights.len() < 145 {
            return Ok(());
        }

        let scale = 1.0 / 127.0;
        let mut normals = Vec::with_capacity(145);

        for logical_row in 0..17 {
            let cols = if logical_row % 2 == 0 { 9 } else { 8 };
            for col in 0..cols {
                let idx = Self::linear_index(col, logical_row);
                let h_center = heights.get(idx).copied().unwrap_or(0.0);

                // Sample neighbors for finite differences
                let (prev_idx, next_idx) = if logical_row % 2 == 0 {
                    // Outer row: neighbors are inner rows above/below or outer col-1/col+1
                    let prev = if logical_row > 0 {
                        Self::linear_index(col.min(7), logical_row - 1)
                    } else {
                        idx
                    };
                    let next = if logical_row < 16 {
                        Self::linear_index(col.min(7), logical_row + 1)
                    } else {
                        idx
                    };
                    (prev, next)
                } else {
                    // Inner row: neighbors are outer rows above/below
                    let prev = Self::linear_index(col, logical_row - 1);
                    let next = if logical_row < 16 {
                        Self::linear_index(col, logical_row + 1)
                    } else {
                        idx
                    };
                    (prev, next)
                };

                let left_idx = if col > 0 {
                    Self::linear_index(col - 1, logical_row)
                } else {
                    idx
                };
                let right_idx = if col + 1 < cols {
                    Self::linear_index(col + 1, logical_row)
                } else {
                    idx
                };

                let h_left = heights.get(left_idx).copied().unwrap_or(h_center);
                let h_right = heights.get(right_idx).copied().unwrap_or(h_center);
                let h_prev = heights.get(prev_idx).copied().unwrap_or(h_center);
                let h_next = heights.get(next_idx).copied().unwrap_or(h_center);

                // Finite differences (UNITSIZE ~ 0.2083 yards per inner vertex step)
                let dz = h_next - h_prev;
                let dx = h_right - h_left;

                // Normal = cross product of tangent vectors, then scale to i8
                // Tangent along X: (1, dx, 0) normalized bias
                // Tangent along Z: (0, dz, 1)
                // Cross: (-dx, 1, -dz) → normalized
                let len = (dx * dx + dz * dz + 1.0).sqrt();
                let nx = (-dx / len / scale).clamp(-127.0, 127.0) as i8;
                let ny = (1.0 / len / scale).clamp(-127.0, 127.0) as i8;
                let nz = (-dz / len / scale).clamp(-127.0, 127.0) as i8;

                // Store in MCNR format: x, z, y (z/y swapped)
                normals.push(wow_adt::chunks::mcnk::VertexNormal {
                    x: nx,
                    z: nz, // Z stored in position 1
                    y: ny, // Y stored in position 2
                });
            }
        }

        let mut padding = vec![0u8; 13];
        // If the chunk already had normals, preserve its padding length
        if let Some(existing) = &chunk.normals {
            padding.resize(existing.padding.len(), 0);
        }

        chunk.normals = Some(wow_adt::chunks::mcnk::McnrChunk { normals, padding });
        Ok(())
    }

    /// Simple raise/lower brush on the 145-vertex grid for chunk `index`.
    ///
    /// `x`, `y` are logical grid coords (0-16 for x, 0-16 for y).
    /// `radius` in grid units. `delta` is the height change.
    /// Marks normals dirty after editing.
    #[wasm_bindgen(js_name = changeTerrain)]
    pub fn change_terrain(
        &mut self,
        index: usize,
        x: usize,
        y: usize,
        radius: f32,
        delta: f32,
    ) -> Result<(), JsError> {
        let chunk = self.chunk_at_mut(index)?;
        let heights = chunk.heights.get_or_insert_with(|| McvtChunk {
            heights: vec![0.0f32; 145],
        });

        if heights.heights.len() < 145 {
            heights.heights.resize(145, 0.0);
        }

        let r2 = radius * radius;

        for logical_row in 0..17 {
            let cols = if logical_row % 2 == 0 { 9 } else { 8 };
            for col in 0..cols {
                // World-ish distance in grid coordinates
                let fy = logical_row as f32;
                let fx = if logical_row % 2 == 0 {
                    col as f32
                } else {
                    col as f32 + 0.5
                };
                let cy = y as f32;
                let cx = if y % 2 == 0 { x as f32 } else { x as f32 + 0.5 };

                let dy = fy - cy;
                let dx = fx - cx;
                let dist2 = dx * dx + dy * dy;

                if dist2 <= r2 {
                    let falloff = 1.0 - (dist2 / r2).sqrt();
                    let linear_idx = Self::linear_index(col, logical_row);
                    if let Some(h) = heights.heights.get_mut(linear_idx) {
                        *h += delta * falloff;
                    }
                }
            }
        }

        chunk.normals = None; // Mark dirty
        Ok(())
    }

    /// Flatten vertices within radius of (x, y) to `target_height`.
    #[wasm_bindgen(js_name = flattenTerrain)]
    pub fn flatten_terrain(
        &mut self,
        index: usize,
        x: usize,
        y: usize,
        radius: f32,
        target_height: f32,
    ) -> Result<(), JsError> {
        let chunk = self.chunk_at_mut(index)?;
        let heights = chunk.heights.get_or_insert_with(|| McvtChunk {
            heights: vec![0.0f32; 145],
        });

        if heights.heights.len() < 145 {
            heights.heights.resize(145, 0.0);
        }

        let r2 = radius * radius;

        for logical_row in 0..17 {
            let cols = if logical_row % 2 == 0 { 9 } else { 8 };
            for col in 0..cols {
                let fy = logical_row as f32;
                let fx = if logical_row % 2 == 0 {
                    col as f32
                } else {
                    col as f32 + 0.5
                };
                let cy = y as f32;
                let cx = if y % 2 == 0 { x as f32 } else { x as f32 + 0.5 };

                let dy = fy - cy;
                let dx = fx - cx;
                if dx * dx + dy * dy <= r2 {
                    let linear_idx = Self::linear_index(col, logical_row);
                    if let Some(h) = heights.heights.get_mut(linear_idx) {
                        *h = target_height;
                    }
                }
            }
        }

        chunk.normals = None;
        Ok(())
    }

    /// Smooth (average) heights within radius around (x, y).
    #[wasm_bindgen(js_name = smoothTerrain)]
    pub fn smooth_terrain(
        &mut self,
        index: usize,
        x: usize,
        y: usize,
        radius: f32,
    ) -> Result<(), JsError> {
        let chunk = self.chunk_at_mut(index)?;
        let heights = match &mut chunk.heights {
            Some(h) => h,
            None => return Ok(()),
        };
        if heights.heights.len() < 145 {
            return Ok(());
        }

        let r2 = radius * radius;
        let mut targets: Vec<(usize, f32)> = Vec::new();

        for logical_row in 0..17 {
            let cols = if logical_row % 2 == 0 { 9 } else { 8 };
            for col in 0..cols {
                let fy = logical_row as f32;
                let fx = if logical_row % 2 == 0 {
                    col as f32
                } else {
                    col as f32 + 0.5
                };
                let cy = y as f32;
                let cx = if y % 2 == 0 { x as f32 } else { x as f32 + 0.5 };

                let dy = fy - cy;
                let dx = fx - cx;
                if dx * dx + dy * dy <= r2 {
                    let linear_idx = Self::linear_index(col, logical_row);
                    // Gather neighboring heights for averaging
                    let mut sum = 0.0f32;
                    let mut count = 0u32;
                    for no in 0..17 {
                        let ncols = if no % 2 == 0 { 9 } else { 8 };
                        for nc in 0..ncols {
                            let nfy = no as f32;
                            let nfx = if no % 2 == 0 {
                                nc as f32
                            } else {
                                nc as f32 + 0.5
                            };
                            let ndy = nfy - fy;
                            let ndx = nfx - fx;
                            if ndx * ndx + ndy * ndy <= r2 {
                                let nidx = Self::linear_index(nc, no);
                                if let Some(nh) = heights.heights.get(nidx) {
                                    sum += *nh;
                                    count += 1;
                                }
                            }
                        }
                    }
                    if count > 0 {
                        targets.push((linear_idx, sum / count as f32));
                    }
                }
            }
        }

        for (idx, avg) in targets {
            if let Some(h) = heights.heights.get_mut(idx) {
                *h = avg;
            }
        }

        chunk.normals = None;
        Ok(())
    }

    /// Reset MCVT heights to zero for chunk `index`.
    #[wasm_bindgen(js_name = clearHeight)]
    pub fn clear_height(&mut self, index: usize) -> Result<(), JsError> {
        let chunk = self.chunk_at_mut(index)?;
        chunk.heights = Some(McvtChunk {
            heights: vec![0.0f32; 145],
        });
        chunk.normals = None;
        Ok(())
    }

    /// Match edge vertices across the 16×16 chunk grid so there are no seams.
    ///
    /// For each pair of adjacent chunks, the shared edge vertices are averaged.
    #[wasm_bindgen(js_name = fixGaps)]
    pub fn fix_gaps(&mut self) -> Result<(), JsError> {
        let n = self.root.mcnk_chunks.len();
        if n < 2 {
            return Ok(());
        }

        // Iterate through chunks; for each, fix the right edge with the chunk to its right,
        // and the bottom edge with the chunk below.
        // The ADT grid is 16×16 normally. Chunk index maps to (px, py) = (idx % 16, idx / 16).
        for idx in 0..n {
            let px = (idx % 16) as i32;
            let py = (idx / 16) as i32;

            // Fix right edge: share vertices with chunk (px+1, py) if it exists
            if px < 15 {
                let right_idx = (py * 16 + (px + 1)) as usize;
                if right_idx < n {
                    Self::fix_edge_right(&mut self.root.mcnk_chunks, idx, right_idx);
                }
            }

            // Fix bottom edge: share vertices with chunk (px, py+1) if it exists
            if py < 15 {
                let bottom_idx = ((py + 1) * 16 + px) as usize;
                if bottom_idx < n {
                    Self::fix_edge_bottom(&mut self.root.mcnk_chunks, idx, bottom_idx);
                }
            }
        }
        Ok(())
    }

    /// Set the low-quality texture map (LOD texture map) for chunk `index`.
    ///
    /// The LOD texture map is a 64-entry uint2 array (one per 8×8 texel
    /// block) indicating the dominant texture for low-detail rendering.
    /// `data` must be a `Uint8Array` of length 64, each value 0-3.
    ///
    /// This updates the `pred_tex` bytes in the MCNK header.
    #[wasm_bindgen(js_name = setLodTextureMap)]
    pub fn set_lod_texture_map(&mut self, index: usize, data: &[u8]) -> Result<(), JsError> {
        if data.len() != 64 {
            return Err(JsError::new("LOD texture map must be exactly 64 bytes"));
        }
        let chunk = self.chunk_at_mut(index)?;
        // Store as 64 uint2 values packed into 16 bytes (two 8-byte arrays).
        // Format: 4 entries per byte, bits [7:6] = entry 0, [5:4] = entry 1, etc.
        // The MCNK header has two [u8; 8] fields: pred_tex and no_effect_doodad.
        // pred_tex stores the first 32 entries (rows 0-3), no_effect_doodad the
        // last 32 entries (rows 4-7). But for simplicity we use pred_tex for all.
        // Actually each byte packs 4 uint2 values; 8 bytes = 32 values.
        // Both fields together give 16 bytes = 64 uint2 values.
        let mut packed_lo = [0u8; 8];
        let mut packed_hi = [0u8; 8];
        for (i, &val) in data.iter().enumerate() {
            if val > 3 {
                return Err(JsError::new(&format!(
                    "LOD entry {} is {}; must be 0-3",
                    i, val
                )));
            }
            if i < 32 {
                let byte_idx = i / 4;
                let bit_shift = (3 - (i % 4)) * 2;
                packed_lo[byte_idx] |= (val & 0x03) << bit_shift;
            } else {
                let j = i - 32;
                let byte_idx = j / 4;
                let bit_shift = (3 - (j % 4)) * 2;
                packed_hi[byte_idx] |= (val & 0x03) << bit_shift;
            }
        }
        chunk.header.pred_tex = packed_lo;
        chunk.header.no_effect_doodad = packed_hi;
        Ok(())
    }

    /// Edit the 4×4 low-res hole map for chunk `index`.
    ///
    /// `add` = true sets a hole, `add` = false clears a hole.
    /// `x`, `y` in 0..4 range.
    #[wasm_bindgen(js_name = setHoles)]
    pub fn set_holes(
        &mut self,
        index: usize,
        x: usize,
        y: usize,
        add: bool,
    ) -> Result<(), JsError> {
        if x >= 4 || y >= 4 {
            return Err(JsError::new("hole coords must be in 0..4"));
        }
        let chunk = self.chunk_at_mut(index)?;
        let bit = y * 4 + x;
        if add {
            chunk.header.holes_low_res |= 1 << bit;
        } else {
            chunk.header.holes_low_res &= !(1 << bit);
        }
        Ok(())
    }

    // ========================================================================
    // Texture painting layer (Task 3)
    // ========================================================================

    /// Brush-based alpha map edit for a specific texture layer.
    ///
    /// Operates on decompressed 64×64 alpha. `layerIndex` is the MCLY layer
    /// (1+ — base layer 0 has no alpha). `x`, `y` in 0..64. `radius` in
    /// texels. `strength` 0.0-1.0.
    #[wasm_bindgen(js_name = paintTexture)]
    pub fn paint_texture(
        &mut self,
        index: usize,
        layer_index: usize,
        x: usize,
        y: usize,
        radius: f32,
        strength: f32,
    ) -> Result<(), JsError> {
        let chunk = self.chunk_at_mut(index)?;

        // Ensure alpha data exists
        let alpha_data = match &mut chunk.alpha {
            Some(a) => &mut a.data,
            None => {
                chunk.alpha = Some(McalChunk::new(vec![0u8; 4096]));
                &mut chunk.alpha.as_mut().unwrap().data
            }
        };

        // Per-layer alpha: 4096 bytes per layer, starting at layer offset
        let layer_alpha_size = 4096usize;
        let layer_offset = layer_index.saturating_sub(1) * layer_alpha_size;
        let needed = layer_offset + layer_alpha_size;
        if alpha_data.len() < needed {
            alpha_data.resize(needed, 0);
        }

        let r2 = radius * radius;
        let target_val = (strength.clamp(0.0, 1.0) * 255.0) as u8;

        for ty in 0..64i32 {
            for tx in 0..64i32 {
                let dy = ty as f32 - y as f32;
                let dx = tx as f32 - x as f32;
                let dist2 = dx * dx + dy * dy;
                if dist2 <= r2 {
                    let falloff = 1.0 - (dist2 / r2).sqrt().clamp(0.0, 1.0);
                    let byte_idx = layer_offset + (ty as usize * 64 + tx as usize);
                    if let Some(pixel) = alpha_data.get_mut(byte_idx) {
                        let blended = (*pixel as f32 * (1.0 - falloff * strength.clamp(0.0, 1.0))
                            + target_val as f32 * falloff * strength.clamp(0.0, 1.0))
                            as u8;
                        *pixel = blended;
                    }
                }
            }
        }

        Self::recalc_mcal_offsets(chunk);
        Ok(())
    }

    /// Convert alpha maps between 32×32 (2048 bytes 4-bit) and 64×64 (4096 bytes 8-bit).
    #[wasm_bindgen(js_name = convertAlphaMap)]
    pub fn convert_alpha_map(&mut self, index: usize, to_big: bool) -> Result<(), JsError> {
        let chunk = self.chunk_at_mut(index)?;
        let alpha = match &mut chunk.alpha {
            Some(a) => a,
            None => return Ok(()),
        };
        let cur = alpha.data.len();
        if to_big && cur < 4096 {
            let mut out = vec![0u8; 4096];
            for (i, &b) in alpha.data.iter().enumerate() {
                let low = b & 0x0F;
                let high = (b >> 4) & 0x0F;
                let base = i * 2;
                if base < 4096 {
                    out[base] = low | (low << 4);
                }
                if base + 1 < 4096 {
                    out[base + 1] = high | (high << 4);
                }
            }
            alpha.data = out;
            chunk.header.flags.value |= 0x8000;
        } else if !to_big && cur >= 4096 {
            let mut out = vec![0u8; 2048];
            for i in 0..2048 {
                let low = (alpha.data[i * 2] >> 4) & 0x0F;
                let high = (alpha.data.get(i * 2 + 1).copied().unwrap_or(0) >> 4) & 0x0F;
                out[i] = (high << 4) | low;
            }
            alpha.data = out;
            chunk.header.flags.value &= !0x8000;
        }
        Ok(())
    }

    /// Convenience: add texture name to MTEX (if not present) and add a layer
    /// to chunk `index`. Returns the texture ID.
    #[wasm_bindgen(js_name = addTextureToChunk)]
    pub fn add_texture_to_chunk(&mut self, index: usize, name: String) -> Result<usize, JsError> {
        let tex_id = match self.root.textures.iter().position(|t| t == &name) {
            Some(id) => id as u32,
            None => {
                self.root.textures.push(name);
                (self.root.textures.len() - 1) as u32
            }
        };
        let chunk = self.chunk_at_mut(index)?;
        let base = chunk.layers.as_ref().map_or(0, |l| l.layers.len());
        let layer = MclyLayer {
            texture_id: tex_id,
            flags: MclyFlags {
                value: if base > 0 { 0x100 } else { 0 },
            },
            offset_in_mcal: 0,
            effect_id: 0,
        };
        match &mut chunk.layers {
            Some(layers) => {
                if layers.layers.len() >= 4 {
                    return Err(JsError::new("chunk already has max 4 layers"));
                }
                layers.layers.push(layer);
                chunk.header.n_layers = layers.layers.len() as u32;
            }
            None => {
                chunk.layers = Some(MclyChunk {
                    layers: vec![layer],
                });
                chunk.header.n_layers = 1;
            }
        }
        Ok(tex_id as usize)
    }

    /// Replace all references to `old_id` with `new_id` across all chunk layers.
    #[wasm_bindgen(js_name = replaceTexture)]
    pub fn replace_texture(&mut self, old_id: usize, new_id: usize) -> Result<(), JsError> {
        let n = self.root.textures.len();
        if old_id >= n || new_id >= n {
            return Err(JsError::new("texture ID out of range"));
        }
        for chunk in &mut self.root.mcnk_chunks {
            if let Some(layers) = &mut chunk.layers {
                for layer in &mut layers.layers {
                    if layer.texture_id as usize == old_id {
                        layer.texture_id = new_id as u32;
                    }
                }
            }
        }
        Ok(())
    }

    /// Set MCLY flags for a layer.
    #[wasm_bindgen(js_name = setTextureFlags)]
    pub fn set_texture_flags(
        &mut self,
        index: usize,
        layer_idx: usize,
        flags: u32,
    ) -> Result<(), JsError> {
        let chunk = self.chunk_at_mut(index)?;
        let layers = chunk
            .layers
            .as_mut()
            .ok_or_else(|| JsError::new("chunk has no texture layers"))?;
        if layer_idx >= layers.layers.len() {
            return Err(JsError::new("layer index out of range"));
        }
        layers.layers[layer_idx].flags = MclyFlags { value: flags };
        Ok(())
    }

    /// Reset MCLY flags to 0 for a layer.
    #[wasm_bindgen(js_name = clearTextureFlags)]
    pub fn clear_texture_flags(&mut self, index: usize, layer_idx: usize) -> Result<(), JsError> {
        self.set_texture_flags(index, layer_idx, 0)
    }

    /// Write shadow bitmap (MCSH) — 512 bytes, 64×64 1-bit.
    #[wasm_bindgen(js_name = setShadows)]
    pub fn set_shadows(&mut self, index: usize, data: &[u8]) -> Result<(), JsError> {
        if data.len() != 512 {
            return Err(JsError::new("shadow map must be exactly 512 bytes"));
        }
        let chunk = self.chunk_at_mut(index)?;
        chunk.shadow = Some(wow_adt::chunks::mcnk::McshChunk {
            shadow_map: data.to_vec(),
        });
        chunk.header.flags.value |= 0x01;
        Ok(())
    }

    /// Write vertex colors (MCCV) — 580 bytes (145 × BGRA).
    #[wasm_bindgen(js_name = setVertexColors)]
    pub fn set_vertex_colors(&mut self, index: usize, data: &[u8]) -> Result<(), JsError> {
        if data.len() != 145 * 4 {
            return Err(JsError::new("vertex colors must be 580 bytes (145 × BGRA)"));
        }
        let chunk = self.chunk_at_mut(index)?;
        let mut colors = Vec::with_capacity(145);
        for i in 0..145 {
            let o = i * 4;
            colors.push(wow_adt::chunks::mcnk::VertexColor {
                b: data[o],
                g: data[o + 1],
                r: data[o + 2],
                a: data[o + 3],
            });
        }
        chunk.vertex_colors = Some(wow_adt::chunks::mcnk::MccvChunk { colors });
        chunk.header.flags.value |= 0x40;
        Ok(())
    }

    // ========================================================================
    // Water editing — additional helpers (Task 4)
    // ========================================================================

    /// Change only the liquid type on an existing MCLQ chunk (Vanilla/TBC).
    #[wasm_bindgen(js_name = setMclqType)]
    pub fn set_mclq_type(&mut self, index: usize, liquid_type: u8) -> Result<(), JsError> {
        let chunk = self.chunk_at_mut(index)?;

        // Try multi-layer first, fall back to single-layer
        let mclq = if let Some(layers) = &mut chunk.liquid_layers {
            layers
                .first_mut()
                .ok_or_else(|| JsError::new("chunk has no MCLQ liquid"))?
        } else {
            chunk
                .liquid
                .as_mut()
                .ok_or_else(|| JsError::new("chunk has no MCLQ liquid"))?
        };

        mclq.liquid_type = match liquid_type {
            0 => LiquidType::Water,
            1 => LiquidType::Ocean,
            2 => LiquidType::Magma,
            3 => LiquidType::Slime,
            _ => return Err(JsError::new("liquidType must be 0-3")),
        };
        // Update MCNK flags
        let flag = match mclq.liquid_type {
            LiquidType::Water => 0x04,
            LiquidType::Ocean => 0x08,
            LiquidType::Magma => 0x10,
            LiquidType::Slime => 0x20,
        };
        chunk.header.flags.value &= !(0x04 | 0x08 | 0x10 | 0x20);
        chunk.header.flags.value |= flag;
        Ok(())
    }

    /// Auto-generate MCLQ water plane from terrain heights.
    ///
    /// Creates a flat water surface at `factor * max_terrain_height`. If the
    /// chunk has no MCLQ yet, one is created.
    #[wasm_bindgen(js_name = autoGenWater)]
    pub fn auto_gen_water(
        &mut self,
        index: usize,
        factor: f32,
        liquid_type: u8,
    ) -> Result<(), JsError> {
        let chunk = self.chunk_at_mut(index)?;
        let max_h = chunk
            .heights
            .as_ref()
            .and_then(|h| {
                h.heights
                    .iter()
                    .cloned()
                    .max_by(|a, b| a.partial_cmp(b).unwrap())
            })
            .unwrap_or(0.0);
        let water_level = max_h * factor;

        let lt = match liquid_type {
            0 => LiquidType::Water,
            1 => LiquidType::Ocean,
            2 => LiquidType::Magma,
            3 => LiquidType::Slime,
            _ => return Err(JsError::new("liquidType must be 0-3")),
        };

        // Build 81 vertices at water_level
        let mut verts = Vec::with_capacity(81);
        for _ in 0..81 {
            verts.push(LiquidVertex {
                union_data: [0u8; 4],
                height: water_level,
            });
        }

        let mclq = MclqChunk {
            min_height: water_level - 1.0,
            max_height: water_level + 1.0,
            vertices: verts,
            tile_flags: [0xFF; 64], // All tiles render
            liquid_type: lt,
        };

        chunk.liquid = Some(mclq.clone());
        chunk.liquid_layers = Some(vec![mclq]);
        let flag = match lt {
            LiquidType::Water => 0x04,
            LiquidType::Ocean => 0x08,
            LiquidType::Magma => 0x10,
            LiquidType::Slime => 0x20,
        };
        chunk.header.flags.value &= !(0x04 | 0x08 | 0x10 | 0x20);
        chunk.header.flags.value |= flag;
        Ok(())
    }

    /// Set MH2O attributes (fishable/deep bitmaps) for a chunk (WotLK+).
    #[wasm_bindgen(js_name = setMh2oAttributes)]
    pub fn set_mh2o_attributes(
        &mut self,
        index: usize,
        fishable: JsValue,
        deep: JsValue,
    ) -> Result<(), JsError> {
        let water = self
            .root
            .water_data
            .as_mut()
            .ok_or_else(|| JsError::new("no MH2O water data; call setMh2o first"))?;
        if index >= water.entries.len() {
            return Err(JsError::new("chunk index out of range"));
        }
        let fish_u64 = parse_u64(&fishable)?;
        let deep_u64 = parse_u64(&deep)?;
        water.entries[index].attributes = Some(Mh2oAttributes {
            fishable: fish_u64,
            deep: deep_u64,
        });
        Ok(())
    }

    /// Set MH2O vertex data for a chunk (WotLK+).
    ///
    /// `data` is a `Float32Array` of heights. The array is mapped into the
    /// 9×9 sparse grid expected by MH2O.
    #[wasm_bindgen(js_name = setMh2oVertexData)]
    pub fn set_mh2o_vertex_data(&mut self, index: usize, data: JsValue) -> Result<(), JsError> {
        let water = self
            .root
            .water_data
            .as_mut()
            .ok_or_else(|| JsError::new("no MH2O water data; call setMh2o first"))?;
        if index >= water.entries.len() {
            return Err(JsError::new("chunk index out of range"));
        }
        let floats = js_sys::Float32Array::new(&data).to_vec();
        let entry = &mut water.entries[index];
        if entry.instances.is_empty() {
            return Err(JsError::new("no MH2O instance; call setMh2o first"));
        }
        let inst = &entry.instances[0];
        let w = inst.width as usize + 1;
        let h = inst.height as usize + 1;

        let mut arr: Box<[Option<HeightDepthVertex>; 81]> = Box::new([const { None }; 81]);
        for z in 0..h {
            for x in 0..w {
                let idx = z * w + x;
                let grid_idx = z * 9 + x;
                if idx < floats.len() && grid_idx < 81 {
                    arr[grid_idx] = Some(HeightDepthVertex {
                        height: floats[idx],
                        depth: 0,
                    });
                }
            }
        }
        entry.vertex_data = vec![Some(VertexDataArray::HeightDepth(arr))];
        Ok(())
    }

    // ========================================================================
    // Object placement editing — additional helpers (Task 5)
    // ========================================================================

    /// Update an existing WMO placement by index.
    ///
    /// `changes` is a plain JS object with any of: `nameId`, `uniqueId`,
    /// `position`, `rotation`, `extentsMin`, `extentsMax`, `flags`,
    /// `doodadSet`, `nameSet`, `scale`.
    #[wasm_bindgen(js_name = updateWmoPlacement)]
    pub fn update_wmo_placement(&mut self, index: usize, changes: JsValue) -> Result<(), JsError> {
        if index >= self.root.wmo_placements.len() {
            return Err(JsError::new("WMO placement index out of range"));
        }
        let p = &mut self.root.wmo_placements[index];
        if let Some(v) = get_opt_u32(&changes, "nameId")? {
            p.name_id = v;
        }
        if let Some(v) = get_opt_u32(&changes, "uniqueId")? {
            p.unique_id = v;
        }
        if let Some(v) = get_opt_f32_array(&changes, "position", 3)? {
            p.position = v;
        }
        if let Some(v) = get_opt_f32_array(&changes, "rotation", 3)? {
            p.rotation = v;
        }
        if let Some(v) = get_opt_f32_array(&changes, "extentsMin", 3)? {
            p.extents_min = v;
        }
        if let Some(v) = get_opt_f32_array(&changes, "extentsMax", 3)? {
            p.extents_max = v;
        }
        if let Some(v) = get_opt_u32(&changes, "flags")? {
            p.flags = v as u16;
        }
        if let Some(v) = get_opt_u32(&changes, "doodadSet")? {
            p.doodad_set = v as u16;
        }
        if let Some(v) = get_opt_u32(&changes, "nameSet")? {
            p.name_set = v as u16;
        }
        if let Some(v) = get_opt_u32(&changes, "scale")? {
            p.scale = v as u16;
        }
        Ok(())
    }

    /// Update an existing doodad placement by index.
    ///
    /// `changes` is a JS object with any of: `nameId`, `uniqueId`, `position`,
    /// `rotation`, `scale`, `flags`.
    #[wasm_bindgen(js_name = updateDoodadPlacement)]
    pub fn update_doodad_placement(
        &mut self,
        index: usize,
        changes: JsValue,
    ) -> Result<(), JsError> {
        if index >= self.root.doodad_placements.len() {
            return Err(JsError::new("doodad placement index out of range"));
        }
        let p = &mut self.root.doodad_placements[index];
        if let Some(v) = get_opt_u32(&changes, "nameId")? {
            p.name_id = v;
        }
        if let Some(v) = get_opt_u32(&changes, "uniqueId")? {
            p.unique_id = v;
        }
        if let Some(v) = get_opt_f32_array(&changes, "position", 3)? {
            p.position = v;
        }
        if let Some(v) = get_opt_f32_array(&changes, "rotation", 3)? {
            p.rotation = v;
        }
        if let Some(v) = get_opt_u32(&changes, "scale")? {
            p.scale = v as u16;
        }
        if let Some(v) = get_opt_u32(&changes, "flags")? {
            p.flags = v as u16;
        }
        Ok(())
    }

    /// Generate a new placement UID that does not conflict with existing ones.
    ///
    /// Scans all WMO and doodad placements in this tile and returns an unused
    /// UID. For a *globally* unique ID across all loaded tiles, the JS side
    /// should maintain a counter or call this on all open tiles and take the
    /// maximum.
    #[wasm_bindgen(js_name = newUid)]
    pub fn new_uid(&self) -> u32 {
        let mut max_id = 0u32;
        for wp in &self.root.wmo_placements {
            max_id = max_id.max(wp.unique_id);
        }
        for dp in &self.root.doodad_placements {
            max_id = max_id.max(dp.unique_id);
        }
        max_id.wrapping_add(1)
    }

    /// Recompute bounding-box extents for a WMO placement.
    ///
    /// Without `wow-wmo-web` available to read the WMO root file, this merely
    /// sets extents to a default 10×10×10 box around the placement position.
    /// The UI should supply proper extents (e.g., by loading the WMO via
    /// `wow-wmo-web` and computing its AABB).
    #[wasm_bindgen(js_name = recalcWmoExtents)]
    pub fn recalc_wmo_extents(&mut self, index: usize) -> Result<(), JsError> {
        if index >= self.root.wmo_placements.len() {
            return Err(JsError::new("WMO placement index out of range"));
        }
        let p = &mut self.root.wmo_placements[index];
        let half = 5.0f32;
        p.extents_min = [
            p.position[0] - half,
            p.position[1] - half,
            p.position[2] - half,
        ];
        p.extents_max = [
            p.position[0] + half,
            p.position[1] + half,
            p.position[2] + half,
        ];
        Ok(())
    }

    // ========================================================================
    // Serialization
    // ========================================================================

    /// Serialize back to a monolithic root ADT file.
    ///
    /// Before serializing this performs a normalization pass:
    /// - Normalizes MCAL alpha layers so cross-layer sums don't exceed 255
    /// - Compresses alpha maps to optimal format (RLE, 4-bit, or 8-bit)
    /// - Auto-generates MCRF references for doodad/WMO placements
    /// - Fixes MH2O `layer_count` to match actual instances
    #[wasm_bindgen(js_name = export)]
    pub fn export(&self) -> Result<js_sys::Uint8Array, JsError> {
        let mut root = self.root.clone();

        // Pre-export normalization
        Self::normalize_alpha_maps(&mut root);
        Self::fix_mh2o_layer_counts(&mut root);

        let built = AdtBuilder::from_parsed(root)
            .build()
            .map_err(|e| JsError::new(&format!("failed to build ADT: {e}")))?;
        let bytes = built
            .to_bytes()
            .map_err(|e| JsError::new(&format!("failed to serialize ADT: {e}")))?;
        Ok(to_uint8_array(&bytes))
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    fn chunk_at(&self, index: usize) -> Result<&wow_adt::chunks::mcnk::McnkChunk, JsError> {
        self.root
            .mcnk_chunks
            .get(index)
            .ok_or_else(|| JsError::new("chunk index out of range"))
    }

    fn chunk_at_mut(
        &mut self,
        index: usize,
    ) -> Result<&mut wow_adt::chunks::mcnk::McnkChunk, JsError> {
        self.root
            .mcnk_chunks
            .get_mut(index)
            .ok_or_else(|| JsError::new("chunk index out of range"))
    }

    /// Convert logical (col, logical_row) to linear index 0..144.
    /// Uses wow-map-viewer indexMapBuf formula: ((y+1)/2)*9 + (y/2)*8 + x
    fn linear_index(col: usize, logical_row: usize) -> usize {
        let y = logical_row;
        y.div_ceil(2) * 9 + (y / 2) * 8 + col
    }

    /// Recompute MCAL offset_in_mcal values for all layers in a chunk.
    /// Each blend layer (1+) gets 4096 bytes; base layer is 0.
    fn recalc_mcal_offsets(chunk: &mut wow_adt::chunks::mcnk::McnkChunk) {
        if let Some(layers) = &mut chunk.layers {
            let mut offset = 0u32;
            for (i, layer) in layers.layers.iter_mut().enumerate() {
                if i == 0 {
                    layer.offset_in_mcal = 0;
                } else {
                    layer.offset_in_mcal = offset;
                    offset += 4096;
                }
            }
        }
    }

    /// Average shared vertices on the right edge of `left_chunk` and left edge of `right_chunk`.
    fn fix_edge_right(
        chunks: &mut [wow_adt::chunks::mcnk::McnkChunk],
        left_idx: usize,
        right_idx: usize,
    ) {
        // Shared vertices are the 17 logical rows' last column(s)
        // Left chunk's right-edge outer rows have col=8; inner rows have col=7
        // Right chunk's left-edge outer rows have col=0; inner rows have col=0
        for logical_row in 0..17 {
            let left_col = if logical_row % 2 == 0 { 8 } else { 7 };
            let right_col = 0usize;
            let li = Self::linear_index(left_col, logical_row);
            let ri = Self::linear_index(right_col, logical_row);

            let (lh, rh) = {
                let left = &chunks[left_idx];
                let right = &chunks[right_idx];
                let lh = left
                    .heights
                    .as_ref()
                    .and_then(|h| h.heights.get(li).copied());
                let rh = right
                    .heights
                    .as_ref()
                    .and_then(|h| h.heights.get(ri).copied());
                (lh, rh)
            };

            if let (Some(lh), Some(rh)) = (lh, rh) {
                let avg = (lh + rh) * 0.5;
                if let Some(ref mut heights) = chunks[left_idx].heights {
                    if let Some(h) = heights.heights.get_mut(li) {
                        *h = avg;
                    }
                }
                if let Some(ref mut heights) = chunks[right_idx].heights {
                    if let Some(h) = heights.heights.get_mut(ri) {
                        *h = avg;
                    }
                }
            }
        }
    }

    /// Average shared vertices on the bottom edge of `top_chunk` and top edge of `bottom_chunk`.
    fn fix_edge_bottom(
        chunks: &mut [wow_adt::chunks::mcnk::McnkChunk],
        top_idx: usize,
        bottom_idx: usize,
    ) {
        // Shared vertices are logical row 16 of top and row 0 of bottom
        // Last row is always outer (logical_row 16), 9 columns
        for col in 0..9 {
            let ti = Self::linear_index(col, 16);
            let bi = Self::linear_index(col, 0);

            let (th, bh) = {
                let top = &chunks[top_idx];
                let bottom = &chunks[bottom_idx];
                let th = top
                    .heights
                    .as_ref()
                    .and_then(|h| h.heights.get(ti).copied());
                let bh = bottom
                    .heights
                    .as_ref()
                    .and_then(|h| h.heights.get(bi).copied());
                (th, bh)
            };

            if let (Some(th), Some(bh)) = (th, bh) {
                let avg = (th + bh) * 0.5;
                if let Some(ref mut heights) = chunks[top_idx].heights {
                    if let Some(h) = heights.heights.get_mut(ti) {
                        *h = avg;
                    }
                }
                if let Some(ref mut heights) = chunks[bottom_idx].heights {
                    if let Some(h) = heights.heights.get_mut(bi) {
                        *h = avg;
                    }
                }
            }
        }
    }

    // ========================================================================
    // Pre-export normalization helpers
    // ========================================================================

    /// Normalize alpha maps: compress bytes and validate cross-layer sums.
    ///
    /// For each chunk with texture layers and alpha data, this:
    /// 1. Splits the raw MCAL bytes into per-layer alpha maps
    /// 2. Normalizes so no texel's cross-layer sum exceeds 255
    /// 3. Recompresses each layer using the optimal format (RLE, 4-bit, 8-bit)
    /// 4. Rewrites the MCAL with compressed data and updates MCLY offsets
    fn normalize_alpha_maps(root: &mut wow_adt::api::RootAdt) {
        use wow_adt::chunks::mcnk::mcal::AlphaMap;

        for chunk in &mut root.mcnk_chunks {
            let layers = match &mut chunk.layers {
                Some(l) => l,
                None => continue,
            };
            let alpha = match &mut chunk.alpha {
                Some(a) => a,
                None => continue,
            };
            let n_layers = layers.layers.len();
            if n_layers < 2 {
                // Only base layer — no alpha maps needed
                chunk.alpha = None;
                continue;
            }

            // Split raw MCAL into per-layer 4096-byte alpha maps
            let blend_count = n_layers - 1;
            let mut per_layer: Vec<Vec<u8>> = Vec::with_capacity(blend_count);
            for i in 0..blend_count {
                let offset = layers.layers[i + 1].offset_in_mcal as usize;
                let end = (offset + 4096).min(alpha.data.len());
                if offset >= alpha.data.len() {
                    per_layer.push(vec![0u8; 4096]);
                } else {
                    let mut buf = vec![0u8; 4096];
                    let copy_len = (end - offset).min(4096);
                    buf[..copy_len].copy_from_slice(&alpha.data[offset..offset + copy_len]);
                    per_layer.push(buf);
                }
            }

            // Normalize: ensure cross-layer sum <= 255 at each texel
            for texel in 0..4096 {
                let mut total: u32 = 0;
                for layer_data in &per_layer {
                    total += layer_data[texel] as u32;
                }
                if total > 255 {
                    // Scale all blend layers proportionally
                    let scale = 255.0 / total as f32;
                    for layer_data in &mut per_layer {
                        layer_data[texel] = (layer_data[texel] as f32 * scale) as u8;
                    }
                }
            }

            // Recompress each layer using optimal format
            let mut new_mcal = Vec::new();
            for (i, layer_data) in per_layer.iter().enumerate() {
                let compressed = AlphaMap::with_optimal_format(layer_data).unwrap_or_else(|_| {
                    AlphaMap::new(
                        layer_data.clone(),
                        wow_adt::chunks::mcnk::mcal::AlphaFormat::Uncompressed4096,
                    )
                });

                let layer_idx = i + 1;
                layers.layers[layer_idx].offset_in_mcal = new_mcal.len() as u32;
                new_mcal.extend_from_slice(&compressed.data);

                // Update MCLY compression flag
                if compressed.format == wow_adt::chunks::mcnk::mcal::AlphaFormat::Compressed {
                    layers.layers[layer_idx].flags.value |= 0x200;
                } else {
                    layers.layers[layer_idx].flags.value &= !0x200;
                }
            }

            alpha.data = new_mcal;
        }
    }

    /// Fix MH2O `layer_count` to match actual instances.
    ///
    /// Ensures each MH2O entry's `header.layer_count` equals the number of
    /// actual instances. If there are no instances, zeros out the header.
    fn fix_mh2o_layer_counts(root: &mut wow_adt::api::RootAdt) {
        if let Some(water) = &mut root.water_data {
            for entry in &mut water.entries {
                let actual = entry.instances.len() as u32;
                if actual == 0 {
                    entry.header.layer_count = 0;
                    entry.header.offset_instances = 0;
                } else if entry.header.layer_count != actual {
                    entry.header.layer_count = actual;
                }
            }
        }
    }
}

// ============================================================================
// JS property extraction helpers
// ============================================================================

fn get_u32(obj: &JsValue, key: &str) -> Result<u32, JsError> {
    let val = js_sys::Reflect::get(obj, &JsValue::from_str(key))
        .map_err(|_| JsError::new(&format!("missing required field: {key}")))?;
    if val.is_undefined() || val.is_null() {
        return Err(JsError::new(&format!("missing required field: {key}")));
    }
    val.as_f64()
        .map(|v| v as u32)
        .ok_or_else(|| JsError::new(&format!("field {key} must be a number")))
}

fn get_opt_u32(obj: &JsValue, key: &str) -> Result<Option<u32>, JsError> {
    let val = match js_sys::Reflect::get(obj, &JsValue::from_str(key)) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    if val.is_undefined() || val.is_null() {
        return Ok(None);
    }
    val.as_f64()
        .map(|v| Some(v as u32))
        .ok_or_else(|| JsError::new(&format!("field {key} must be a number")))
}

fn get_f32(obj: &JsValue, key: &str) -> Result<f32, JsError> {
    let val = js_sys::Reflect::get(obj, &JsValue::from_str(key))
        .map_err(|_| JsError::new(&format!("missing required field: {key}")))?;
    if val.is_undefined() || val.is_null() {
        return Err(JsError::new(&format!("missing required field: {key}")));
    }
    val.as_f64()
        .map(|v| v as f32)
        .ok_or_else(|| JsError::new(&format!("field {key} must be a number")))
}

fn get_f32_array(obj: &JsValue, key: &str, expected: usize) -> Result<[f32; 3], JsError> {
    let val = js_sys::Reflect::get(obj, &JsValue::from_str(key))
        .map_err(|_| JsError::new(&format!("missing required field: {key}")))?;
    if val.is_undefined() || val.is_null() {
        return Err(JsError::new(&format!("missing required field: {key}")));
    }
    let arr = js_sys::Float32Array::new(&val);
    if arr.length() as usize != expected {
        return Err(JsError::new(&format!(
            "field {key} must have {expected} elements"
        )));
    }
    let mut result = [0.0f32; 3];
    let vec = arr.to_vec();
    result.copy_from_slice(&vec[..expected]);
    Ok(result)
}

fn get_opt_f32_array(
    obj: &JsValue,
    key: &str,
    expected: usize,
) -> Result<Option<[f32; 3]>, JsError> {
    let val = match js_sys::Reflect::get(obj, &JsValue::from_str(key)) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    if val.is_undefined() || val.is_null() {
        return Ok(None);
    }
    let arr = js_sys::Float32Array::new(&val);
    if arr.length() as usize != expected {
        return Err(JsError::new(&format!(
            "field {key} must have {expected} elements"
        )));
    }
    let mut result = [0.0f32; 3];
    let vec = arr.to_vec();
    result.copy_from_slice(&vec[..expected.min(vec.len())]);
    Ok(Some(result))
}

fn get_opt_u64_str(obj: &JsValue, key: &str) -> Result<Option<u64>, JsError> {
    let val = match js_sys::Reflect::get(obj, &JsValue::from_str(key)) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    if val.is_undefined() || val.is_null() {
        return Ok(None);
    }
    if let Some(n) = val.as_f64() {
        return Ok(Some(n as u64));
    }
    // Try string
    if let Some(s) = val.as_string() {
        let parsed = s.parse::<u64>().map_err(|_| {
            JsError::new(&format!("field {key} must be a number or numeric string"))
        })?;
        return Ok(Some(parsed));
    }
    Err(JsError::new(&format!(
        "field {key} must be a number or numeric string"
    )))
}

/// Parse a JsValue that can be a number or a numeric string into u64.
fn parse_u64(v: &JsValue) -> Result<u64, JsError> {
    if let Some(n) = v.as_f64() {
        return Ok(n as u64);
    }
    if let Some(s) = v.as_string() {
        return s
            .parse::<u64>()
            .map_err(|_| JsError::new("expected a number or numeric string"));
    }
    Err(JsError::new("expected a number or numeric string"))
}
