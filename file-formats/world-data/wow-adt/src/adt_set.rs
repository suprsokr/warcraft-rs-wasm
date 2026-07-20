//! High-level API for loading complete ADT split file sets.
//!
//! This module provides convenient APIs for working with Cataclysm+ split ADT files
//! as a logical unit, handling discovery, loading, and merging automatically.
//!
//! ## Quick Start
//!
//! ```no_run
//! use std::path::Path;
//! use wow_adt::adt_set::AdtSet;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Load complete split file set
//! let adt_set = AdtSet::load_from_path("Azeroth_30_30.adt")?;
//!
//! // Merge into unified structure
//! let merged = adt_set.merge()?;
//!
//! println!("Merged {} MCNK chunks", merged.mcnk_chunks.len());
//! println!("Textures: {}", merged.textures.len());
//! println!("Models: {}", merged.models.len());
//! # Ok(())
//! # }
//! ```
//!
//! ## References
//!
//! - [wowdev.wiki ADT/v18](https://wowdev.wiki/ADT/v18) - Split file architecture
//! - `SPLIT_FILE_ARCHITECTURE.md` - Detailed documentation

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use crate::api::{LodAdt, Obj0Adt, RootAdt, Tex0Adt};
use crate::error::Result;
use crate::merger::merge_split_files;
use crate::split_set::SplitFileSet;
use crate::{ParsedAdt, parse_adt};

fn wrong_file_type(details: &str) -> crate::error::AdtError {
    crate::error::AdtError::ChunkParseError {
        chunk: crate::chunk_id::ChunkId::MVER,
        offset: 0,
        details: details.to_string(),
    }
}

/// Parse bytes as a root ADT, erroring on other file types.
fn parse_root(bytes: &[u8]) -> Result<RootAdt> {
    let mut cursor = Cursor::new(bytes);
    match parse_adt(&mut cursor)? {
        ParsedAdt::Root(r) => Ok(*r),
        _ => Err(wrong_file_type(
            "Expected root ADT file but got different file type",
        )),
    }
}

/// Parse bytes as a `_tex0` file; `None` if it's a different file type.
fn parse_tex0(bytes: &[u8]) -> Result<Option<Tex0Adt>> {
    let mut cursor = Cursor::new(bytes);
    Ok(match parse_adt(&mut cursor)? {
        ParsedAdt::Tex0(t) => Some(t),
        _ => None,
    })
}

/// Parse bytes as an `_obj0` file; `None` if it's a different file type.
fn parse_obj0(bytes: &[u8]) -> Result<Option<Obj0Adt>> {
    let mut cursor = Cursor::new(bytes);
    Ok(match parse_adt(&mut cursor)? {
        ParsedAdt::Obj0(o) => Some(o),
        _ => None,
    })
}

/// Parse bytes as a `_lod` file; `None` if it's a different file type.
fn parse_lod(bytes: &[u8]) -> Result<Option<LodAdt>> {
    let mut cursor = Cursor::new(bytes);
    Ok(match parse_adt(&mut cursor)? {
        ParsedAdt::Lod(l) => Some(l),
        _ => None,
    })
}

/// Complete set of parsed ADT split files.
///
/// Represents all files that make up a Cataclysm+ ADT tile, with automatic
/// discovery and loading from the filesystem.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use wow_adt::adt_set::AdtSet;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Load complete split file set
/// let adt_set = AdtSet::load_from_path("Azeroth_30_30.adt")?;
///
/// // Check what files were loaded
/// println!("Has texture file: {}", adt_set.texture.is_some());
/// println!("Has object file: {}", adt_set.object.is_some());
/// println!("Has LOD file: {}", adt_set.lod.is_some());
///
/// // Merge into unified structure
/// let merged = adt_set.merge()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct AdtSet {
    /// Root ADT with terrain geometry
    pub root: RootAdt,

    /// Texture file (optional but expected for Cataclysm+)
    pub texture: Option<Tex0Adt>,

    /// Object file (optional but expected for Cataclysm+)
    pub object: Option<Obj0Adt>,

    /// LOD file (optional, Legion+ only)
    pub lod: Option<LodAdt>,
}

impl AdtSet {
    /// Load complete ADT set from filesystem.
    ///
    /// Given a root ADT file path, this method automatically discovers and loads
    /// all associated split files (texture, object, LOD). Files that don't exist
    /// are simply omitted (set to `None`).
    ///
    /// # Arguments
    ///
    /// * `root_path` - Path to the root ADT file (e.g., "Azeroth_30_30.adt")
    ///
    /// # Returns
    ///
    /// `AdtSet` with root file and any present split files loaded.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Root file doesn't exist or can't be read
    /// - Any present split file can't be parsed
    /// - File format is invalid
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use wow_adt::adt_set::AdtSet;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // Load from filesystem
    /// let adt_set = AdtSet::load_from_path("World/Maps/Azeroth/Azeroth_30_30.adt")?;
    ///
    /// // Root is always present
    /// assert_eq!(adt_set.root.mcnk_chunks.len(), 256);
    ///
    /// // Split files may or may not be present
    /// if adt_set.texture.is_some() && adt_set.object.is_some() {
    ///     println!("Complete Cataclysm+ split file set");
    /// } else {
    ///     println!("Pre-Cataclysm monolithic file");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn load_from_path(root_path: impl AsRef<Path>) -> Result<Self> {
        let root_path = root_path.as_ref();

        // Discover split file paths
        let file_set = SplitFileSet::discover(root_path);

        // Load root (required)
        let root = parse_root(&fs::read(&file_set.root)?)?;

        // Load split files (optional)
        let load_optional = |path: Option<&PathBuf>| -> Result<Option<Vec<u8>>> {
            match path {
                Some(p) if p.exists() => Ok(Some(fs::read(p)?)),
                _ => Ok(None),
            }
        };
        let texture = load_optional(file_set.tex0.as_ref())?
            .map(|b| parse_tex0(&b))
            .transpose()?
            .flatten();
        let object = load_optional(file_set.obj0.as_ref())?
            .map(|b| parse_obj0(&b))
            .transpose()?
            .flatten();
        let lod = load_optional(file_set.lod.as_ref())?
            .map(|b| parse_lod(&b))
            .transpose()?
            .flatten();

        Ok(AdtSet {
            root,
            texture,
            object,
            lod,
        })
    }

    /// Load a complete ADT set from in-memory bytes, without a filesystem.
    ///
    /// Given the root ADT's virtual name (used only to derive split file
    /// names like `<stem>_tex0.adt`) and its raw bytes, this method calls
    /// `resolve` for each expected split file name. The resolver returns
    /// the file's bytes, or `None` when the file is absent (which simply
    /// omits that part of the set, like a missing file on disk).
    ///
    /// This is the filesystem-free counterpart to
    /// [`load_from_path`](AdtSet::load_from_path), intended for wasm
    /// environments and non-filesystem sources (MPQ archives, network
    /// fetches, in-memory maps).
    ///
    /// # Arguments
    ///
    /// * `root_name` - Virtual name of the root ADT (e.g.,
    ///   `"World/Maps/Azeroth/Azeroth_30_30.adt"`)
    /// * `root_bytes` - Raw bytes of the root ADT file
    /// * `resolve` - Callback mapping a derived split file name to its
    ///   bytes, or `None` if that file does not exist
    ///
    /// # Errors
    ///
    /// Returns error if the root bytes or any resolved split file can't
    /// be parsed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::collections::HashMap;
    /// use wow_adt::adt_set::AdtSet;
    ///
    /// # fn example(root: Vec<u8>, tex0: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
    /// let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    /// files.insert("Azeroth_30_30_tex0.adt".to_string(), tex0);
    ///
    /// let adt_set = AdtSet::from_named_bytes("Azeroth_30_30.adt", &root, |name| {
    ///     files.get(name).cloned()
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_named_bytes(
        root_name: &str,
        root_bytes: &[u8],
        resolve: impl Fn(&str) -> Option<Vec<u8>>,
    ) -> Result<Self> {
        let file_set = SplitFileSet::discover(root_name);
        let name_of = |path: &PathBuf| path.to_string_lossy().into_owned();

        let root = parse_root(root_bytes)?;

        let texture = file_set
            .tex0
            .as_ref()
            .and_then(|p| resolve(&name_of(p)))
            .map(|b| parse_tex0(&b))
            .transpose()?
            .flatten();
        let object = file_set
            .obj0
            .as_ref()
            .and_then(|p| resolve(&name_of(p)))
            .map(|b| parse_obj0(&b))
            .transpose()?
            .flatten();
        let lod = file_set
            .lod
            .as_ref()
            .and_then(|p| resolve(&name_of(p)))
            .map(|b| parse_lod(&b))
            .transpose()?
            .flatten();

        Ok(AdtSet {
            root,
            texture,
            object,
            lod,
        })
    }

    /// Merge split files into unified RootAdt structure.
    ///
    /// Combines data from texture and object files into the root ADT, producing
    /// a unified structure that matches the pre-Cataclysm monolithic format.
    ///
    /// # Returns
    ///
    /// Merged `RootAdt` with all data from split files combined.
    ///
    /// # Errors
    ///
    /// Returns error if MCNK chunk counts don't match between files.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use wow_adt::adt_set::AdtSet;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let adt_set = AdtSet::load_from_path("Azeroth_30_30.adt")?;
    ///
    /// // Merge into unified structure
    /// let merged = adt_set.merge()?;
    ///
    /// // Access all data from single structure
    /// println!("MCNK chunks: {}", merged.mcnk_chunks.len());
    /// println!("Textures: {}", merged.textures.len());
    /// println!("Models: {}", merged.models.len());
    /// # Ok(())
    /// # }
    /// ```
    pub fn merge(self) -> Result<RootAdt> {
        merge_split_files(self.root, self.texture, self.object)
    }

    /// Check if this represents a complete Cataclysm+ split file set.
    ///
    /// A complete set requires root, texture, and object files.
    ///
    /// # Returns
    ///
    /// `true` if texture and object files are present.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use wow_adt::adt_set::AdtSet;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let adt_set = AdtSet::load_from_path("Azeroth_30_30.adt")?;
    ///
    /// if adt_set.is_complete() {
    ///     println!("Complete Cataclysm+ split file set");
    /// } else {
    ///     println!("Incomplete or pre-Cataclysm file");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn is_complete(&self) -> bool {
        self.texture.is_some() && self.object.is_some()
    }

    /// Get ADT version from root file.
    ///
    /// # Returns
    ///
    /// The WoW client version this ADT was created for.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use wow_adt::adt_set::AdtSet;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let adt_set = AdtSet::load_from_path("Azeroth_30_30.adt")?;
    /// println!("ADT version: {:?}", adt_set.version());
    /// # Ok(())
    /// # }
    /// ```
    pub fn version(&self) -> crate::version::AdtVersion {
        self.root.version
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::McnkChunk;
    use crate::version::AdtVersion;

    fn create_test_mcnk() -> McnkChunk {
        // Use unsafe zeroed for simplicity in tests
        unsafe { std::mem::zeroed() }
    }

    fn create_test_root() -> RootAdt {
        RootAdt {
            version: AdtVersion::Cataclysm,
            mhdr: Default::default(),
            mcin: Default::default(),
            textures: vec![],
            models: vec![],
            model_indices: vec![],
            wmos: vec![],
            wmo_indices: vec![],
            doodad_placements: vec![],
            wmo_placements: vec![],
            mcnk_chunks: vec![create_test_mcnk(), create_test_mcnk()],
            flight_bounds: None,
            water_data: None,
            texture_flags: None,
            texture_amplifier: None,
            texture_params: None,
            blend_mesh_headers: None,
            blend_mesh_bounds: None,
            blend_mesh_vertices: None,
            blend_mesh_indices: None,
        }
    }

    fn create_test_tex() -> Tex0Adt {
        Tex0Adt {
            version: AdtVersion::Cataclysm,
            textures: vec!["texture1.blp".to_string()],
            texture_params: None,
            mcnk_textures: vec![],
        }
    }

    fn create_test_obj() -> Obj0Adt {
        Obj0Adt {
            version: AdtVersion::Cataclysm,
            models: vec!["model1.m2".to_string()],
            model_indices: vec![0],
            wmos: vec![],
            wmo_indices: vec![],
            doodad_placements: vec![],
            wmo_placements: vec![],
            mcnk_objects: vec![],
        }
    }

    #[test]
    fn test_adt_set_complete() {
        let adt_set = AdtSet {
            root: create_test_root(),
            texture: Some(create_test_tex()),
            object: Some(create_test_obj()),
            lod: None,
        };

        assert!(adt_set.is_complete());
        assert_eq!(adt_set.version(), AdtVersion::Cataclysm);
    }

    #[test]
    fn test_adt_set_incomplete() {
        let adt_set = AdtSet {
            root: create_test_root(),
            texture: Some(create_test_tex()),
            object: None,
            lod: None,
        };

        assert!(!adt_set.is_complete());
    }

    #[test]
    fn test_adt_set_merge() {
        let adt_set = AdtSet {
            root: create_test_root(),
            texture: Some(create_test_tex()),
            object: Some(create_test_obj()),
            lod: None,
        };

        let result = adt_set.merge();
        assert!(result.is_ok());

        let merged = result.unwrap();
        assert_eq!(merged.textures.len(), 1);
        assert_eq!(merged.models.len(), 1);
    }

    #[test]
    fn test_adt_set_merge_no_optional() {
        let adt_set = AdtSet {
            root: create_test_root(),
            texture: None,
            object: None,
            lod: None,
        };

        let result = adt_set.merge();
        assert!(result.is_ok());

        let merged = result.unwrap();
        assert_eq!(merged.textures.len(), 0);
        assert_eq!(merged.models.len(), 0);
    }
}
