//! Web (wasm-bindgen) bindings for the `wow-wmo` WMO (World Model Object)
//! library.
//!
//! A WMO is one root file plus N group files (`name.wmo`,
//! `name_000.wmo`, …). Everything happens in memory: bytes in
//! (`Uint8Array`), plain data out. Group geometry (the heavy part) is
//! loaded lazily through a resolver callback and exposed as typed arrays.
//!
//! ```js
//! import init, { WmoFile } from "wow-wmo-web";
//! await init();
//!
//! const wmo = WmoFile.loadWithGroups("dungeon.wmo", rootBytes, (name) =>
//!   archive.hasFile(name) ? archive.readFile(name) : undefined,
//! );
//! console.log(wmo.summary());
//! const verts = wmo.groupVertices(0);  // Float32Array (x,y,z interleaved)
//! ```

use std::collections::HashMap;
use std::io::Cursor;

use serde::Serialize;
use wasm_bindgen::prelude::*;
use wow_web_common::{to_js, to_uint8_array};
use wow_wmo::version::WmoVersion;
use wow_wmo::wmo_types::{WmoHeader, WmoRoot};
use wow_wmo::{BoundingBox, Color, ParsedWmo, Vec3, WmoEditor, WmoFlags, WmoParser, parse_wmo};

/// Accepts an expansion name: "classic", "tbc", "wotlk", "cata", "mop",
/// "wod", "legion", "bfa", "shadowlands", "dragonflight", "warwithin".
fn parse_version(s: &str) -> Result<WmoVersion, JsError> {
    match s.to_ascii_lowercase().as_str() {
        "classic" | "vanilla" => Ok(WmoVersion::Classic),
        "tbc" => Ok(WmoVersion::Tbc),
        "wotlk" | "wrath" => Ok(WmoVersion::Wotlk),
        "cata" | "cataclysm" => Ok(WmoVersion::Cataclysm),
        "mop" => Ok(WmoVersion::Mop),
        "wod" => Ok(WmoVersion::Wod),
        "legion" => Ok(WmoVersion::Legion),
        "bfa" => Ok(WmoVersion::Bfa),
        "shadowlands" | "sl" => Ok(WmoVersion::Shadowlands),
        "dragonflight" | "df" => Ok(WmoVersion::Dragonflight),
        "warwithin" | "tww" => Ok(WmoVersion::WarWithin),
        _ => Err(JsError::new(&format!(
            "invalid version \"{s}\" (use an expansion name like \"wotlk\")"
        ))),
    }
}

fn version_name(version: WmoVersion) -> &'static str {
    match version {
        WmoVersion::Classic => "classic",
        WmoVersion::Tbc => "tbc",
        WmoVersion::Wotlk => "wotlk",
        WmoVersion::Cataclysm => "cataclysm",
        WmoVersion::Mop => "mop",
        WmoVersion::Wod => "wod",
        WmoVersion::Legion => "legion",
        WmoVersion::Bfa => "bfa",
        WmoVersion::Shadowlands => "shadowlands",
        WmoVersion::Dragonflight => "dragonflight",
        WmoVersion::WarWithin => "warwithin",
    }
}

/// Derive the conventional group file name: `stem.wmo` → `stem_000.wmo`.
fn group_file_name(stem: &str, index: usize) -> String {
    format!("{stem}_{index:03}.wmo")
}

/// Strip a trailing `.wmo` extension (case-insensitive) from a name,
/// keeping any directory prefix.
fn file_stem(name: &str) -> &str {
    name.strip_suffix(".wmo")
        .or_else(|| name.strip_suffix(".WMO"))
        .unwrap_or(name)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GroupSummary {
    index: usize,
    name: String,
    flags: u32,
    bounding_box_min: [f32; 3],
    bounding_box_max: [f32; 3],
    /// Whether group file bytes have been loaded (geometry available).
    loaded: bool,
    vertex_count: u32,
    triangle_count: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WmoSummary {
    version: String,
    group_count: usize,
    loaded_group_count: usize,
    textures: Vec<String>,
    material_count: usize,
    portal_count: usize,
    light_count: usize,
    doodad_def_count: usize,
    doodad_set_count: usize,
    bounding_box_min: [f32; 3],
    bounding_box_max: [f32; 3],
    flags: u32,
    skybox: Option<String>,
    groups: Vec<GroupSummary>,
}

fn vec3_to_arr(v: &Vec3) -> [f32; 3] {
    [v.x, v.y, v.z]
}

/// A WMO root file, plus any group files loaded via
/// [`loadWithGroups`](Wmo::load_with_groups).
///
/// Group files are parsed read-only (geometry exposed as typed arrays);
/// the root can be re-serialized with [`exportRoot()`](Wmo::export_root).
#[wasm_bindgen(js_name = WmoFile)]
pub struct Wmo {
    editor: WmoEditor,
    /// Parsed group geometry (sparse; indexed by group index).
    groups: HashMap<usize, wow_wmo::group_parser::WmoGroup>,
}

#[wasm_bindgen(js_class = WmoFile)]
impl Wmo {
    /// Open a WMO root file from raw bytes. Pass group bytes to
    /// [`loadGroup()`](Wmo::load_group), or use `loadWithGroups`.
    #[wasm_bindgen(constructor)]
    pub fn open(data: &[u8]) -> Result<Wmo, JsError> {
        let mut cursor = Cursor::new(data);
        let root = WmoParser::new()
            .parse_root(&mut cursor)
            .map_err(|e| JsError::new(&format!("failed to parse WMO root: {e}")))?;
        Ok(Wmo {
            editor: WmoEditor::new(root),
            groups: HashMap::new(),
        })
    }

    /// Create a new, empty WMO root file. `version` is an expansion name
    /// ("classic", "tbc", "wotlk", "cata", "mop", …).
    #[wasm_bindgen(js_name = create)]
    pub fn create(version: &str) -> Result<Wmo, JsError> {
        let version = parse_version(version)?;
        let root = WmoRoot {
            version,
            materials: Vec::new(),
            groups: Vec::new(),
            portals: Vec::new(),
            portal_references: Vec::new(),
            visible_block_lists: Vec::new(),
            lights: Vec::new(),
            doodad_defs: Vec::new(),
            doodad_sets: Vec::new(),
            bounding_box: BoundingBox {
                min: Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                max: Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            },
            textures: Vec::new(),
            texture_offset_index_map: HashMap::new(),
            header: WmoHeader {
                n_materials: 0,
                n_groups: 0,
                n_portals: 0,
                n_lights: 0,
                n_doodad_names: 0,
                n_doodad_defs: 0,
                n_doodad_sets: 0,
                flags: WmoFlags::empty(),
                ambient_color: Color::default(),
            },
            skybox: None,
            convex_volume_planes: None,
        };
        Ok(Wmo {
            editor: WmoEditor::new(root),
            groups: HashMap::new(),
        })
    }

    /// Open a root file and load all group files through a resolver
    /// callback `(name) => Uint8Array | null | undefined`. Group names are
    /// derived conventionally: `<stem>_000.wmo`, `<stem>_001.wmo`, …
    /// A missing group file (resolver returns undefined) is an error.
    #[wasm_bindgen(js_name = loadWithGroups)]
    pub fn load_with_groups(
        root_name: &str,
        root_bytes: &[u8],
        resolver: js_sys::Function,
    ) -> Result<Wmo, JsError> {
        let mut wmo = Wmo::open(root_bytes)?;
        let stem = file_stem(root_name);
        for index in 0..wmo.editor.root().groups.len() {
            let name = group_file_name(stem, index);
            let value = resolver
                .call1(&JsValue::NULL, &JsValue::from_str(&name))
                .map_err(|e| JsError::new(&format!("resolver threw for \"{name}\": {e:?}")))?;
            if value.is_null() || value.is_undefined() {
                return Err(JsError::new(&format!(
                    "group file \"{name}\" is required but the resolver returned nothing"
                )));
            }
            let bytes = js_sys::Uint8Array::new(&value).to_vec();
            wmo.parse_group_bytes(index, &bytes)
                .map_err(|e| JsError::new(&format!("failed to parse \"{name}\": {e:?}")))?;
        }
        Ok(wmo)
    }

    fn parse_group_bytes(&mut self, index: usize, bytes: &[u8]) -> Result<(), JsError> {
        let mut cursor = Cursor::new(bytes);
        match parse_wmo(&mut cursor).map_err(|e| JsError::new(&format!("{e}")))? {
            ParsedWmo::Group(group) => {
                self.groups.insert(index, group);
                Ok(())
            }
            ParsedWmo::Root(_) => Err(JsError::new("expected a group file, got a root file")),
        }
    }

    /// Load a single group file from bytes. `index` is the group index
    /// (0-based, matching the `_000` suffix order).
    #[wasm_bindgen(js_name = loadGroup)]
    pub fn load_group(&mut self, index: usize, bytes: &[u8]) -> Result<(), JsError> {
        if index >= self.editor.root().groups.len() {
            return Err(JsError::new("group index out of range"));
        }
        self.parse_group_bytes(index, bytes)
    }

    /// Summary of the root file: version, counts, textures, bounding box,
    /// and per-group metadata (including whether geometry is loaded).
    #[wasm_bindgen(js_name = summary)]
    pub fn summary(&self) -> Result<JsValue, JsError> {
        let root = self.editor.root();
        let groups = root
            .groups
            .iter()
            .enumerate()
            .map(|(i, g)| {
                let loaded = self.groups.get(&i);
                GroupSummary {
                    index: i,
                    name: g.name.clone(),
                    flags: g.flags.bits(),
                    bounding_box_min: vec3_to_arr(&g.bounding_box.min),
                    bounding_box_max: vec3_to_arr(&g.bounding_box.max),
                    loaded: loaded.is_some(),
                    vertex_count: loaded.map_or(0, |gr| gr.n_vertices),
                    triangle_count: loaded.map_or(0, |gr| gr.n_triangles),
                }
            })
            .collect();
        to_js(&WmoSummary {
            version: version_name(root.version).to_string(),
            group_count: root.groups.len(),
            loaded_group_count: self.groups.len(),
            textures: root.textures.clone(),
            material_count: root.materials.len(),
            portal_count: root.portals.len(),
            light_count: root.lights.len(),
            doodad_def_count: root.doodad_defs.len(),
            doodad_set_count: root.doodad_sets.len(),
            bounding_box_min: vec3_to_arr(&root.bounding_box.min),
            bounding_box_max: vec3_to_arr(&root.bounding_box.max),
            flags: root.header.flags.bits(),
            skybox: root.skybox.clone(),
            groups,
        })
    }

    /// Number of groups declared by the root file.
    #[wasm_bindgen(js_name = groupCount)]
    pub fn group_count(&self) -> u32 {
        self.editor.root().groups.len() as u32
    }

    fn loaded_group(&self, index: usize) -> Result<&wow_wmo::group_parser::WmoGroup, JsError> {
        self.groups.get(&index).ok_or_else(|| {
            JsError::new("group not loaded (use loadGroup/loadWithGroups) or index out of range")
        })
    }

    /// Vertex positions of a loaded group: `Float32Array` with
    /// interleaved `[x, y, z]` per vertex.
    #[wasm_bindgen(js_name = groupVertices)]
    pub fn group_vertices(&self, index: usize) -> Result<js_sys::Float32Array, JsError> {
        let group = self.loaded_group(index)?;
        let flat: Vec<f32> = group
            .vertex_positions
            .iter()
            .flat_map(|v| [v.x, v.y, v.z])
            .collect();
        Ok(js_sys::Float32Array::from(flat.as_slice()))
    }

    /// Vertex normals of a loaded group: `Float32Array` with interleaved
    /// `[x, y, z]` per vertex.
    #[wasm_bindgen(js_name = groupNormals)]
    pub fn group_normals(&self, index: usize) -> Result<js_sys::Float32Array, JsError> {
        let group = self.loaded_group(index)?;
        let flat: Vec<f32> = group
            .vertex_normals
            .iter()
            .flat_map(|n| [n.x, n.y, n.z])
            .collect();
        Ok(js_sys::Float32Array::from(flat.as_slice()))
    }

    /// Texture coordinates of a loaded group: `Float32Array` with
    /// interleaved `[u, v]` per vertex.
    #[wasm_bindgen(js_name = groupTexCoords)]
    pub fn group_tex_coords(&self, index: usize) -> Result<js_sys::Float32Array, JsError> {
        let group = self.loaded_group(index)?;
        let flat: Vec<f32> = group
            .texture_coords
            .iter()
            .flat_map(|t| [t.u, t.v])
            .collect();
        Ok(js_sys::Float32Array::from(flat.as_slice()))
    }

    /// Triangle indices of a loaded group (`Uint16Array`, 3 per triangle).
    #[wasm_bindgen(js_name = groupIndices)]
    pub fn group_indices(&self, index: usize) -> Result<js_sys::Uint16Array, JsError> {
        let group = self.loaded_group(index)?;
        Ok(js_sys::Uint16Array::from(group.vertex_indices.as_slice()))
    }

    /// Add a texture filename to the root (MOTX).
    #[wasm_bindgen(js_name = addTexture)]
    pub fn add_texture(&mut self, filename: &str) {
        self.editor.add_texture(filename.to_string());
    }

    /// Serialize the root file back to WMO bytes (group files are not
    /// writable yet — they are parse-only in the underlying library).
    #[wasm_bindgen(js_name = exportRoot)]
    pub fn export_root(&self) -> Result<js_sys::Uint8Array, JsError> {
        let mut out = Cursor::new(Vec::new());
        self.editor
            .save_root(&mut out)
            .map_err(|e| JsError::new(&format!("failed to write WMO root: {e}")))?;
        Ok(to_uint8_array(&out.into_inner()))
    }
}
