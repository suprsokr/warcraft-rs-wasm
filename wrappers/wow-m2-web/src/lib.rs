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
use wow_m2::animation::{
    AnimationManager, AnimationManagerBuilder, BoneTransformComputer, Mat4, Vec3,
};
use wow_m2::skin::SkinFile;
use wow_m2::{parse_m2, M2Format, M2Model};
use wow_web_common::{set_property, to_js, to_uint8_array};

/// Installs a panic hook so Rust panics print a useful message to the browser
/// console instead of just `RuntimeError: unreachable`.
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

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
struct AnimationInfo {
    index: usize,
    id: u16,
    sub_id: u16,
    /// Duration in milliseconds.
    duration: u32,
    flags: u32,
}

/// One render batch resolved for drawing (see [`M2Renderer::batches`]).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchInfo {
    /// Index of the submesh (geoset) this batch draws.
    submesh: usize,
    /// Geoset id of the submesh (used for LOD/skin grouping).
    submesh_id: u16,
    /// Resolved texture index into the model's `textures` list (or -1).
    texture_index: i32,
    /// Raw `texture_combo_index` from the batch.
    texture_combo_index: u16,
    /// Blend mode (0 opaque, 1 alpha-key, 2 alpha, 4 add, 5 mod, 6 mod2x, ...).
    blend_mode: u16,
    /// True if backface culling should be disabled.
    two_sided: bool,
    /// True if the batch is unlit (draw at full brightness).
    unlit: bool,
    /// True if the depth buffer should not be written (transparent passes).
    no_depth_write: bool,
    /// Start offset (in indices) into the shared index buffer.
    index_start: u32,
    /// Number of indices for this batch (3 per triangle).
    index_count: u32,
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

    /// List of animation sequences for a UI picker.
    ///
    /// Returns an array of `{ index, id, subId, duration, flags }`. The
    /// `id` maps to WoW's animation slot (0 = Stand, 4 = Walk, ...); the
    /// JS side turns it into a human-readable label.
    #[wasm_bindgen(js_name = animations)]
    pub fn animations(&self) -> Result<JsValue, JsError> {
        let model = self.format.model();
        let list: Vec<AnimationInfo> = model
            .animations
            .iter()
            .enumerate()
            .map(|(index, seq)| AnimationInfo {
                index,
                id: seq.animation_id,
                sub_id: seq.sub_animation_id,
                duration: seq
                    .end_timestamp
                    .map(|end| end.saturating_sub(seq.start_timestamp))
                    .unwrap_or(seq.start_timestamp),
                flags: seq.flags,
            })
            .collect();
        to_js(&list)
    }

    /// The texture lookup table (`texture_combo_index` -> texture index).
    #[wasm_bindgen(js_name = textureLookupTable)]
    pub fn texture_lookup_table(&self) -> js_sys::Uint16Array {
        js_sys::Uint16Array::from(self.format.model().raw_data.texture_lookup_table.as_slice())
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

/// A ready-to-draw M2 model with animation playback and CPU skinning.
///
/// This bundles the parsed model, its render skin (external `.skin` for
/// WotLK+, or the embedded skin for older models), and an animation
/// manager. It produces GPU-friendly buffers:
///
/// - a shared, de-duplicated **index buffer** whose ranges line up with
///   the batches returned by [`batches`](M2Renderer::batches), and
/// - per-frame CPU-skinned **vertex positions and normals** in OpenGL
///   `Y-up` space (WoW is `Z-up`, so `(x, y, z) -> (x, z, -y)`).
///
/// ```js
/// import init, { M2Renderer } from "wow-m2-web";
/// await init();
/// const r = new M2Renderer(m2Bytes, skinBytes); // skinBytes may be null
/// const batches = r.batches();
/// const indices = r.indices();
/// r.setAnimation(0);
/// // each frame:
/// r.update(dtMs);
/// const positions = r.skinnedVertices();
/// const normals = r.skinnedNormals();
/// ```
#[wasm_bindgen(js_name = M2Renderer)]
pub struct M2Renderer {
    model: M2Model,
    manager: AnimationManager,
    computer: BoneTransformComputer,
    /// Shared element buffer; batch ranges index into this.
    indices: Vec<u32>,
    /// One [`BatchInfo`] per drawn batch (parallel to index ranges).
    batches: Vec<BatchInfo>,
    /// Cached base (bind-pose) positions in WoW model space.
    ///
    /// Bone pivots and animation tracks are also in WoW space, so CPU
    /// skinning must happen in this coordinate system. We convert the final
    /// skinned output to GL space when writing `skinned_positions`.
    base_positions: Vec<[f32; 3]>,
    /// Cached base normals in WoW model space.
    base_normals: Vec<[f32; 3]>,
    /// Cached per-vertex bone indices/weights.
    bone_indices: Vec<[u8; 4]>,
    bone_weights: Vec<[u8; 4]>,
    /// Scratch skinned output buffers (flat x,y,z interleaved).
    skinned_positions: Vec<f32>,
    skinned_normals: Vec<f32>,
}

/// Convert a WoW `Z-up` position/direction to OpenGL `Y-up`.
#[inline]
fn to_gl(x: f32, y: f32, z: f32) -> [f32; 3] {
    [x, z, -y]
}

#[inline]
fn vec3_to_gl(v: Vec3) -> [f32; 3] {
    to_gl(v.x, v.y, v.z)
}

#[wasm_bindgen(js_class = M2Renderer)]
impl M2Renderer {
    /// Create a renderer from M2 bytes and optional external `.skin` bytes.
    ///
    /// If `skin_data` is `null`/empty the model's embedded skin is used
    /// (pre-WotLK models). Textures are resolved by the caller and bound
    /// per batch using [`batches`](M2Renderer::batches).
    #[wasm_bindgen(constructor)]
    pub fn new(m2_data: &[u8], skin_data: Option<Vec<u8>>) -> Result<M2Renderer, JsError> {
        let format = parse_m2(&mut Cursor::new(m2_data))
            .map_err(|e| JsError::new(&format!("failed to parse M2: {e}")))?;
        let model = format.model().clone();

        // Resolve the render skin: prefer an external .skin, fall back to
        // the embedded skin data for older (pre-WotLK) models, which store
        // their skin profiles inside the .m2 itself.
        let skin = match skin_data {
            Some(bytes) if !bytes.is_empty() => SkinFile::parse(&mut Cursor::new(&bytes))
                .map_err(|e| JsError::new(&format!("failed to parse skin: {e}")))?,
            _ => model.parse_embedded_skin(m2_data, 0).map_err(|e| {
                JsError::new(&format!(
                    "no external skin provided and embedded skin parse failed: {e}"
                ))
            })?,
        };

        // Build the animation manager (resolves bone tracks from raw bytes).
        let manager = AnimationManagerBuilder::from_model(&model, m2_data)
            .unwrap_or_else(|_| AnimationManager::empty());

        // Build the bone transform computer.
        let pivots: Vec<Vec3> = model
            .bones
            .iter()
            .map(|b| Vec3::new(b.pivot.x, b.pivot.y, b.pivot.z))
            .collect();
        let parents: Vec<i16> = model.bones.iter().map(|b| b.parent_bone).collect();
        let flags: Vec<u32> = model.bones.iter().map(|b| b.flags.bits()).collect();
        let computer = if pivots.is_empty() {
            BoneTransformComputer::empty()
        } else {
            BoneTransformComputer::new(&pivots, &parents, &flags)
        };

        // Cache base geometry in WoW space plus per-vertex skinning data.
        //
        // Bone pivots and animation tracks are stored in WoW coordinates. If
        // we convert vertices to GL space before applying the bone matrices,
        // the animation math mixes coordinate systems and character models
        // explode. Keep skinning inputs in WoW space and convert only the
        // final skinned output to GL space.
        let base_positions: Vec<[f32; 3]> = model
            .vertices
            .iter()
            .map(|v| [v.position.x, v.position.y, v.position.z])
            .collect();
        let base_normals: Vec<[f32; 3]> = model
            .vertices
            .iter()
            .map(|v| [v.normal.x, v.normal.y, v.normal.z])
            .collect();
        let bone_indices: Vec<[u8; 4]> = model.vertices.iter().map(|v| v.bone_indices).collect();
        let bone_weights: Vec<[u8; 4]> = model.vertices.iter().map(|v| v.bone_weights).collect();

        // Build the shared index buffer and batch ranges.
        let triangles = skin.triangles();
        let submeshes = skin.submeshes();
        let tex_lookup = &model.raw_data.texture_lookup_table;

        let mut indices: Vec<u32> = Vec::new();
        let mut batches: Vec<BatchInfo> = Vec::new();

        for batch in skin.batches() {
            let sm_idx = batch.skin_section_index as usize;
            let Some(sm) = submeshes.get(sm_idx) else {
                continue;
            };

            let tri_start = sm.triangle_start as usize;
            let tri_count = sm.triangle_count as usize;
            let end = (tri_start + tri_count).min(triangles.len());
            if tri_start >= end {
                continue;
            }

            let index_start = indices.len() as u32;
            for &t in &triangles[tri_start..end] {
                indices.push(t as u32);
            }
            let index_count = indices.len() as u32 - index_start;

            // Resolve texture: texture_combo_index -> texture_lookup_table -> textures.
            let texture_index = tex_lookup
                .get(batch.texture_combo_index as usize)
                .map(|&i| i as i32)
                .unwrap_or(-1);

            let material = model.materials.get(batch.material_index as usize);
            let (two_sided, unlit, no_depth_write, blend_mode) = match material {
                Some(m) => (
                    m.flags
                        .contains(wow_m2::chunks::material::M2RenderFlags::NO_BACKFACE_CULLING),
                    m.flags
                        .contains(wow_m2::chunks::material::M2RenderFlags::UNLIT),
                    !m.flags
                        .contains(wow_m2::chunks::material::M2RenderFlags::DEPTH_WRITE),
                    m.blend_mode.bits(),
                ),
                None => (false, false, false, 0),
            };

            batches.push(BatchInfo {
                submesh: sm_idx,
                submesh_id: sm.id,
                texture_index,
                texture_combo_index: batch.texture_combo_index,
                blend_mode,
                two_sided,
                unlit,
                no_depth_write,
                index_start,
                index_count,
            });
        }

        let vert_count = base_positions.len();
        Ok(M2Renderer {
            model,
            manager,
            computer,
            indices,
            batches,
            base_positions,
            base_normals,
            bone_indices,
            bone_weights,
            skinned_positions: vec![0.0; vert_count * 3],
            skinned_normals: vec![0.0; vert_count * 3],
        })
    }

    /// Number of vertices.
    #[wasm_bindgen(js_name = vertexCount)]
    pub fn vertex_count(&self) -> usize {
        self.base_positions.len()
    }

    /// The shared triangle index buffer (`Uint32Array`). Batch ranges
    /// returned by [`batches`](M2Renderer::batches) index into this.
    #[wasm_bindgen(js_name = indices)]
    pub fn indices(&self) -> js_sys::Uint32Array {
        js_sys::Uint32Array::from(self.indices.as_slice())
    }

    /// Per-batch draw information (texture, blend, cull, index range).
    #[wasm_bindgen(js_name = batches)]
    pub fn batches(&self) -> Result<JsValue, JsError> {
        to_js(&self.batches)
    }

    /// Texture metadata (`textureType`, `flags`, `filename`) — the same
    /// list referenced by `BatchInfo.textureIndex`.
    #[wasm_bindgen(js_name = textures)]
    pub fn textures(&self) -> Result<JsValue, JsError> {
        let list: Vec<TextureInfo> = self
            .model
            .textures
            .iter()
            .map(|t| TextureInfo {
                texture_type: t.texture_type as u32,
                flags: t.flags.bits(),
                filename: t.filename.string.to_string_lossy(),
            })
            .collect();
        to_js(&list)
    }

    /// Animation sequences for a UI picker (`{index,id,subId,duration,flags}`).
    #[wasm_bindgen(js_name = animations)]
    pub fn animations(&self) -> Result<JsValue, JsError> {
        let list: Vec<AnimationInfo> = self
            .model
            .animations
            .iter()
            .enumerate()
            .map(|(index, seq)| AnimationInfo {
                index,
                id: seq.animation_id,
                sub_id: seq.sub_animation_id,
                duration: seq
                    .end_timestamp
                    .map(|end| end.saturating_sub(seq.start_timestamp))
                    .unwrap_or(seq.start_timestamp),
                flags: seq.flags,
            })
            .collect();
        to_js(&list)
    }

    /// Axis-aligned bounding box in GL space: `[minX,minY,minZ,maxX,maxY,maxZ]`.
    #[wasm_bindgen(js_name = boundingBox)]
    pub fn bounding_box(&self) -> js_sys::Float32Array {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in &self.base_positions {
            let gl_p = to_gl(p[0], p[1], p[2]);
            for i in 0..3 {
                min[i] = min[i].min(gl_p[i]);
                max[i] = max[i].max(gl_p[i]);
            }
        }
        if !self.base_positions.is_empty() {
            js_sys::Float32Array::from([min[0], min[1], min[2], max[0], max[1], max[2]].as_slice())
        } else {
            js_sys::Float32Array::from([-1.0f32, -1.0, -1.0, 1.0, 1.0, 1.0].as_slice())
        }
    }

    /// Set the current animation by sequence index (into `animations()`).
    #[wasm_bindgen(js_name = setAnimation)]
    pub fn set_animation(&mut self, index: usize) {
        self.manager.set_animation_index(index);
    }

    /// Advance the animation by `delta_ms` and recompute bone transforms.
    #[wasm_bindgen(js_name = update)]
    pub fn update(&mut self, delta_ms: f64) {
        if self.manager.bone_count() == 0 {
            return;
        }
        self.manager.update(delta_ms);

        let bone_count = self.computer.bone_count();
        let mut translations = Vec::with_capacity(bone_count);
        let mut rotations = Vec::with_capacity(bone_count);
        let mut scales = Vec::with_capacity(bone_count);
        for i in 0..bone_count {
            translations.push(self.manager.get_bone_translation(i));
            rotations.push(self.manager.get_bone_rotation(i));
            scales.push(self.manager.get_bone_scale(i));
        }
        self.computer.update(&translations, &rotations, &scales);
        self.skin_now();
    }

    /// CPU-skinned vertex positions for the current frame (`Float32Array`,
    /// interleaved x,y,z). Call [`update`](M2Renderer::update) first.
    #[wasm_bindgen(js_name = skinnedVertices)]
    pub fn skinned_vertices(&self) -> js_sys::Float32Array {
        js_sys::Float32Array::from(self.skinned_positions.as_slice())
    }

    /// CPU-skinned vertex normals for the current frame (`Float32Array`).
    #[wasm_bindgen(js_name = skinnedNormals)]
    pub fn skinned_normals(&self) -> js_sys::Float32Array {
        js_sys::Float32Array::from(self.skinned_normals.as_slice())
    }

    /// Primary texture coordinates (`Float32Array`, interleaved u,v).
    /// These are static (texture animations are not yet applied).
    #[wasm_bindgen(js_name = texCoords)]
    pub fn tex_coords(&self) -> js_sys::Float32Array {
        let flat: Vec<f32> = self
            .model
            .vertices
            .iter()
            .flat_map(|v| [v.tex_coords.x, v.tex_coords.y])
            .collect();
        js_sys::Float32Array::from(flat.as_slice())
    }

    /// Compute skinned positions/normals into the scratch buffers.
    fn skin_now(&mut self) {
        let bones = self.computer.bones();
        let has_bones = !bones.is_empty();

        for (vi, ((pos, nrm), (bidx, bwt))) in self
            .base_positions
            .iter()
            .zip(self.base_normals.iter())
            .zip(self.bone_indices.iter().zip(self.bone_weights.iter()))
            .enumerate()
        {
            let out_p = vi * 3;

            if !has_bones {
                let gl_pos = to_gl(pos[0], pos[1], pos[2]);
                let gl_nrm = to_gl(nrm[0], nrm[1], nrm[2]);
                self.skinned_positions[out_p] = gl_pos[0];
                self.skinned_positions[out_p + 1] = gl_pos[1];
                self.skinned_positions[out_p + 2] = gl_pos[2];
                self.skinned_normals[out_p] = gl_nrm[0];
                self.skinned_normals[out_p + 1] = gl_nrm[1];
                self.skinned_normals[out_p + 2] = gl_nrm[2];
                continue;
            }

            let base_pos = Vec3::new(pos[0], pos[1], pos[2]);
            let base_nrm = Vec3::new(nrm[0], nrm[1], nrm[2]);
            let mut acc_p = Vec3::ZERO;
            let mut acc_n = Vec3::ZERO;
            let mut total_w = 0.0f32;

            for k in 0..4 {
                let w = bwt[k] as f32 / 255.0;
                if w <= 0.0 {
                    continue;
                }
                let bone = bidx[k] as usize;
                let Some(cb) = bones.get(bone) else {
                    continue;
                };
                let m: &Mat4 = &cb.post_billboard_transform;
                let tp = m.transform_point(base_pos);
                let tn = m.transform_normal(base_nrm);
                acc_p.x += tp.x * w;
                acc_p.y += tp.y * w;
                acc_p.z += tp.z * w;
                acc_n.x += tn.x * w;
                acc_n.y += tn.y * w;
                acc_n.z += tn.z * w;
                total_w += w;
            }

            if total_w <= 0.0 {
                // Unweighted vertex: leave in bind pose.
                acc_p = base_pos;
                acc_n = base_nrm;
            }

            let gl_pos = vec3_to_gl(acc_p);
            self.skinned_positions[out_p] = gl_pos[0];
            self.skinned_positions[out_p + 1] = gl_pos[1];
            self.skinned_positions[out_p + 2] = gl_pos[2];

            let len = (acc_n.x * acc_n.x + acc_n.y * acc_n.y + acc_n.z * acc_n.z).sqrt();
            let final_nrm = if len > 1e-6 {
                Vec3::new(acc_n.x / len, acc_n.y / len, acc_n.z / len)
            } else {
                base_nrm
            };
            let gl_nrm = vec3_to_gl(final_nrm);
            self.skinned_normals[out_p] = gl_nrm[0];
            self.skinned_normals[out_p + 1] = gl_nrm[1];
            self.skinned_normals[out_p + 2] = gl_nrm[2];
        }
    }
}
