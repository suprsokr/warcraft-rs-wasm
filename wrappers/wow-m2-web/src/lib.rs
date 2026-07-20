//! Web (wasm-bindgen) bindings for the `wow-m2` M2 model library.
//!
//! M2 models reference external `.skin` and `.anim` files; those are
//! parsed by separate classes (`M2Skin`, `M2Anim`) from bytes the caller
//! supplies — the JS side resolves companion files itself (e.g. from a
//! `wow-mpq-web` archive), which keeps parsing fully in memory.
//!
//! ```js
//! import init, { M2File, M2Skin } from "wow-m2-web";
//! await init();
//!
//! const model = new M2File(mainBytes);
//! console.log(model.summary());
//! const skin = M2Skin.parse(archive.readFile("model.skin"));
//! const positions = model.vertices();  // Float32Array (x,y,z interleaved)
//! ```

use std::io::Cursor;

use serde::Serialize;
use wasm_bindgen::prelude::*;
use wow_m2::anim::AnimParser;
use wow_m2::skin::SkinFile;
use wow_m2::{M2Format, parse_m2};
use wow_web_common::{set_property, to_js, to_uint8_array};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TextureInfo {
    texture_type: u32,
    flags: u32,
    filename: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct M2Summary {
    name: Option<String>,
    /// "legacy" (pre-Legion MD20) or "chunked" (Legion+ MD21).
    format: &'static str,
    /// Raw version field from the header (e.g. 264 for WotLK).
    version: u32,
    vertex_count: usize,
    bone_count: usize,
    animation_count: usize,
    texture_count: usize,
    material_count: usize,
    particle_emitter_count: usize,
    ribbon_emitter_count: usize,
    attachment_count: usize,
    event_count: usize,
    textures: Vec<TextureInfo>,
    /// FileDataIDs of external .skin files (chunked/Legion+ models).
    skin_file_ids: Vec<u32>,
    /// FileDataIDs of external .anim files (chunked/Legion+ models).
    animation_file_ids: Vec<u32>,
    /// FileDataIDs of external texture files (chunked/Legion+ models).
    texture_file_ids: Vec<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SkinSummary {
    /// "new" (WotLK+ external .skin) or "old" (pre-WotLK style).
    format: &'static str,
    index_count: usize,
    triangle_count: usize,
    submesh_count: usize,
    batch_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AnimSummary {
    /// "modern" (Legion+, MAOF header) or "legacy".
    format: &'static str,
    section_count: usize,
}

fn anim_format_name(format: &wow_m2::AnimFormat) -> &'static str {
    match format {
        wow_m2::AnimFormat::Modern => "modern",
        wow_m2::AnimFormat::Legacy => "legacy",
    }
}

/// An M2 model parsed in memory.
#[wasm_bindgen(js_name = M2File)]
pub struct M2 {
    format: M2Format,
}

#[wasm_bindgen(js_class = M2File)]
impl M2 {
    /// Parse an M2 model from raw bytes (both legacy MD20 and chunked
    /// MD21 files are auto-detected).
    #[wasm_bindgen(constructor)]
    pub fn open(data: &[u8]) -> Result<M2, JsError> {
        let mut cursor = Cursor::new(data);
        let format =
            parse_m2(&mut cursor).map_err(|e| JsError::new(&format!("failed to parse M2: {e}")))?;
        Ok(M2 { format })
    }

    /// Create a new, empty M2 model (legacy format, version 264).
    #[wasm_bindgen(js_name = create)]
    pub fn create() -> M2 {
        M2 {
            format: M2Format::Legacy(wow_m2::M2Model::default()),
        }
    }

    /// Summary: name, format, version, counts, texture list and external
    /// file references (FileDataIDs for chunked models).
    #[wasm_bindgen(js_name = summary)]
    pub fn summary(&self) -> Result<JsValue, JsError> {
        let model = self.format.model();
        to_js(&M2Summary {
            name: model.name.clone(),
            format: if self.format.is_chunked() {
                "chunked"
            } else {
                "legacy"
            },
            version: model.header.version,
            vertex_count: model.vertices.len(),
            bone_count: model.bones.len(),
            animation_count: model.animations.len(),
            texture_count: model.textures.len(),
            material_count: model.materials.len(),
            particle_emitter_count: model.particle_emitters.len(),
            ribbon_emitter_count: model.ribbon_emitters.len(),
            attachment_count: model.attachments.len(),
            event_count: model.events.len(),
            textures: model
                .textures
                .iter()
                .map(|t| TextureInfo {
                    texture_type: t.texture_type as u32,
                    flags: t.flags.bits(),
                    filename: t.filename.string.to_string_lossy(),
                })
                .collect(),
            skin_file_ids: model
                .skin_file_ids
                .as_ref()
                .map(|s| s.ids.clone())
                .unwrap_or_default(),
            animation_file_ids: model
                .animation_file_ids
                .as_ref()
                .map(|a| a.ids.clone())
                .unwrap_or_default(),
            texture_file_ids: model
                .texture_file_ids
                .as_ref()
                .map(|t| t.ids.clone())
                .unwrap_or_default(),
        })
    }

    /// Vertex positions: `Float32Array` with interleaved `[x, y, z]`.
    #[wasm_bindgen(js_name = vertices)]
    pub fn vertices(&self) -> js_sys::Float32Array {
        let flat: Vec<f32> = self
            .format
            .model()
            .vertices
            .iter()
            .flat_map(|v| [v.position.x, v.position.y, v.position.z])
            .collect();
        js_sys::Float32Array::from(flat.as_slice())
    }

    /// Vertex normals: `Float32Array` with interleaved `[x, y, z]`.
    #[wasm_bindgen(js_name = normals)]
    pub fn normals(&self) -> js_sys::Float32Array {
        let flat: Vec<f32> = self
            .format
            .model()
            .vertices
            .iter()
            .flat_map(|v| [v.normal.x, v.normal.y, v.normal.z])
            .collect();
        js_sys::Float32Array::from(flat.as_slice())
    }

    /// Primary texture coordinates: `Float32Array` with interleaved
    /// `[u, v]`.
    #[wasm_bindgen(js_name = texCoords)]
    pub fn tex_coords(&self) -> js_sys::Float32Array {
        let flat: Vec<f32> = self
            .format
            .model()
            .vertices
            .iter()
            .flat_map(|v| [v.tex_coords.x, v.tex_coords.y])
            .collect();
        js_sys::Float32Array::from(flat.as_slice())
    }

    /// Skinning data per vertex: bone weights and bone indices, each a
    /// `Uint8Array` with 4 entries per vertex.
    #[wasm_bindgen(js_name = skinWeights)]
    pub fn skin_weights(&self) -> Result<JsValue, JsError> {
        let model = self.format.model();
        let weights: Vec<u8> = model.vertices.iter().flat_map(|v| v.bone_weights).collect();
        let indices: Vec<u8> = model.vertices.iter().flat_map(|v| v.bone_indices).collect();
        let obj = js_sys::Object::new();
        set_property(&obj, "boneWeights", to_uint8_array(&weights).into())?;
        set_property(&obj, "boneIndices", to_uint8_array(&indices).into())?;
        Ok(obj.into())
    }

    /// Serialize the model back to M2 bytes.
    #[wasm_bindgen(js_name = export)]
    pub fn export(&self) -> Result<js_sys::Uint8Array, JsError> {
        let mut out = Cursor::new(Vec::new());
        self.format
            .model()
            .write(&mut out)
            .map_err(|e| JsError::new(&format!("failed to write M2: {e}")))?;
        Ok(to_uint8_array(&out.into_inner()))
    }
}

/// An external `.skin` file (submeshes and render batches for an M2).
#[wasm_bindgen(js_name = M2Skin)]
pub struct Skin {
    file: SkinFile,
}

#[wasm_bindgen(js_class = M2Skin)]
impl Skin {
    /// Parse a `.skin` file from raw bytes (old and new formats are
    /// auto-detected).
    #[wasm_bindgen(js_name = parse)]
    pub fn parse(data: &[u8]) -> Result<Skin, JsError> {
        let mut cursor = Cursor::new(data);
        let file = SkinFile::parse(&mut cursor)
            .map_err(|e| JsError::new(&format!("failed to parse skin: {e}")))?;
        Ok(Skin { file })
    }

    /// Summary: format, index/triangle/submesh/batch counts.
    #[wasm_bindgen(js_name = summary)]
    pub fn summary(&self) -> Result<JsValue, JsError> {
        let (format, indices, triangles, submeshes, batches) = match &self.file {
            SkinFile::New(s) => (
                "new",
                s.indices.len(),
                s.triangles.len(),
                s.submeshes.len(),
                s.batches.len(),
            ),
            SkinFile::Old(s) => (
                "old",
                s.indices.len(),
                s.triangles.len(),
                s.submeshes.len(),
                s.batches.len(),
            ),
        };
        to_js(&SkinSummary {
            format,
            index_count: indices,
            triangle_count: triangles,
            submesh_count: submeshes,
            batch_count: batches,
        })
    }

    /// Triangle vertex indices for rendering (`Uint16Array`, 3 per
    /// triangle).
    #[wasm_bindgen(js_name = triangles)]
    pub fn triangles(&self) -> js_sys::Uint16Array {
        let tris = match &self.file {
            SkinFile::New(s) => &s.triangles,
            SkinFile::Old(s) => &s.triangles,
        };
        js_sys::Uint16Array::from(tris.as_slice())
    }

    /// Serialize back to `.skin` bytes.
    #[wasm_bindgen(js_name = export)]
    pub fn export(&self) -> Result<js_sys::Uint8Array, JsError> {
        let mut out = Cursor::new(Vec::new());
        self.file
            .write(&mut out)
            .map_err(|e| JsError::new(&format!("failed to write skin: {e}")))?;
        Ok(to_uint8_array(&out.into_inner()))
    }
}

/// An external `.anim` file (animation sequences for an M2).
#[wasm_bindgen(js_name = M2Anim)]
pub struct Anim {
    file: wow_m2::AnimFile,
}

#[wasm_bindgen(js_class = M2Anim)]
impl Anim {
    /// Parse an `.anim` file from raw bytes (legacy and modern formats
    /// are auto-detected).
    #[wasm_bindgen(js_name = parse)]
    pub fn parse(data: &[u8]) -> Result<Anim, JsError> {
        let mut cursor = Cursor::new(data);
        let file = AnimParser::parse(&mut cursor)
            .map_err(|e| JsError::new(&format!("failed to parse anim: {e}")))?;
        Ok(Anim { file })
    }

    /// Summary: format and section count.
    #[wasm_bindgen(js_name = summary)]
    pub fn summary(&self) -> Result<JsValue, JsError> {
        to_js(&AnimSummary {
            format: anim_format_name(&self.file.format),
            section_count: self.file.sections.len(),
        })
    }

    /// Serialize back to `.anim` bytes.
    #[wasm_bindgen(js_name = export)]
    pub fn export(&self) -> Result<js_sys::Uint8Array, JsError> {
        let mut out = Cursor::new(Vec::new());
        self.file
            .write(&mut out)
            .map_err(|e| JsError::new(&format!("failed to write anim: {e}")))?;
        Ok(to_uint8_array(&out.into_inner()))
    }
}
