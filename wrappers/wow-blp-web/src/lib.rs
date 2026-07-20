//! Web (wasm-bindgen) bindings for the `wow-blp` BLP texture library.
//!
//! Decode and encode BLP textures from JavaScript — everything happens in
//! memory: bytes in (`Uint8Array`), pixels/bytes out. No filesystem needed.
//!
//! ```js
//! import init, { decodeBlp, encodeBlp, blpToPng, pngToBlp } from "wow-blp-web";
//! await init();
//!
//! const tex = decodeBlp(new Uint8Array(await file.arrayBuffer()));
//! console.log(tex.width, tex.height, tex.mipmaps);
//! // tex.rgba is a Uint8Array of RGBA pixels for the main mipmap level
//!
//! const png = blpToPng(bytes);          // BLP -> PNG bytes
//! const blp = pngToBlp(png, { format: "dxt5" }); // PNG -> BLP bytes
//! ```

use serde::Deserialize;
use wasm_bindgen::prelude::*;
use wow_blp::convert::{
    AlphaBits, Blp2Format, BlpOldFormat, BlpTarget, DxtAlgorithm, FilterType, blp_to_image,
    image_to_blp,
};
use wow_blp::encode::encode_blp;
use wow_blp::parser::parse_blp;
use wow_blp::types::{BlpImage, BlpVersion, CompressionType};
use wow_web_common::{set_property, to_js, to_uint8_array};

/// Information about one mipmap level of a decoded BLP.
#[derive(serde::Serialize)]
struct MipmapEntry {
    level: usize,
    width: u32,
    height: u32,
    /// Size of the compressed data for this level, in bytes.
    data_size: usize,
}

fn version_name(version: BlpVersion) -> &'static str {
    match version {
        BlpVersion::Blp0 => "blp0",
        BlpVersion::Blp1 => "blp1",
        BlpVersion::Blp2 => "blp2",
    }
}

fn compression_name(compression: CompressionType) -> &'static str {
    match compression {
        CompressionType::Jpeg => "jpeg",
        CompressionType::Raw1 => "raw1",
        CompressionType::Raw3 => "raw3",
        CompressionType::Dxt1 => "dxt1",
        CompressionType::Dxt3 => "dxt3",
        CompressionType::Dxt5 => "dxt5",
    }
}

fn parse(data: &[u8]) -> Result<BlpImage, JsError> {
    // Note: BLP0 files with *external* mipmaps cannot be fully decoded
    // in-memory; parse_blp only handles self-contained data. This is rare
    // (Warcraft III RoC beta) — BLP1/BLP2 always embed their mipmaps.
    parse_blp(data).map_err(|e| {
        JsError::new(&format!(
            "failed to parse BLP (BLP0 files with external mipmap files are not supported): {e}"
        ))
    })
}

fn decode_level(
    image: &BlpImage,
    mipmap_level: Option<u32>,
) -> Result<(image::RgbaImage, u32, u32), JsError> {
    let level = mipmap_level.unwrap_or(0) as usize;
    let rgba = blp_to_image(image, level)
        .map_err(|e| JsError::new(&format!("failed to decode mipmap level {level}: {e}")))?
        .to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok((rgba, w, h))
}

fn mipmaps_of(image: &BlpImage) -> Vec<MipmapEntry> {
    image
        .mipmap_info()
        .into_iter()
        .map(|m| MipmapEntry {
            level: m.level,
            width: m.width,
            height: m.height,
            data_size: m.data_size,
        })
        .collect()
}

/// Decode a BLP texture and return its metadata plus the RGBA pixels of the
/// requested mipmap level (default 0).
///
/// Returns
/// `{ width, height, rgba, mipmaps, version, compression, alphaBits }`
/// where `rgba` is a `Uint8Array` of `width * height * 4` bytes.
#[wasm_bindgen(js_name = decodeBlp)]
pub fn decode_blp(data: &[u8], mipmap_level: Option<u32>) -> Result<JsValue, JsError> {
    let image = parse(data)?;
    let (rgba, width, height) = decode_level(&image, mipmap_level)?;

    let obj = js_sys::Object::new();
    let set = |key: &str, value: JsValue| -> Result<(), JsError> { set_property(&obj, key, value) };
    set("width", JsValue::from(width))?;
    set("height", JsValue::from(height))?;
    set("rgba", to_uint8_array(rgba.as_raw()).into())?;
    set("mipmaps", to_js(&mipmaps_of(&image))?)?;
    set(
        "version",
        JsValue::from_str(version_name(image.header.version)),
    )?;
    set(
        "compression",
        JsValue::from_str(compression_name(image.compression_type())),
    )?;
    set("alphaBits", JsValue::from(image.alpha_bit_depth()))?;
    Ok(obj.into())
}

/// Options accepted by [`encodeBlp`](encode_blp_js) and [`pngToBlp`](png_to_blp).
#[derive(Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct EncodeOptions {
    /// BLP version: "blp1" (War3 TFT) or "blp2" (WoW). Default "blp2".
    version: String,
    /// Encoding: "raw1" (palettized), "raw3" (uncompressed RGBA, blp2 only),
    /// "jpeg", "dxt1", "dxt3" or "dxt5" (dxt* are blp2 only).
    /// Default "dxt5" for blp2, "jpeg" for blp1.
    format: Option<String>,
    /// Alpha bits for "raw1": 0, 1, 4 or 8. Default 8.
    alpha_bits: u8,
    /// Whether jpeg/dxt encodings keep the alpha channel. Default true.
    has_alpha: bool,
    /// DXTn compression quality: "fast" (range fit), "balanced" (cluster
    /// fit) or "best" (iterative cluster fit). Default "balanced".
    quality: String,
    /// Generate mipmaps. Default true.
    mipmaps: bool,
    /// Mipmap downscale filter: "nearest", "triangle", "catmullRom",
    /// "gaussian" or "lanczos3". Default "triangle".
    filter: String,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        EncodeOptions {
            version: "blp2".into(),
            format: None,
            alpha_bits: 8,
            has_alpha: true,
            quality: "balanced".into(),
            mipmaps: true,
            filter: "triangle".into(),
        }
    }
}

impl EncodeOptions {
    fn alpha_bits(&self) -> Result<AlphaBits, JsError> {
        match self.alpha_bits {
            0 => Ok(AlphaBits::NoAlpha),
            1 => Ok(AlphaBits::Bit1),
            4 => Ok(AlphaBits::Bit4),
            8 => Ok(AlphaBits::Bit8),
            other => Err(JsError::new(&format!(
                "alphaBits must be 0, 1, 4 or 8, got {other}"
            ))),
        }
    }

    fn dxt_algorithm(&self) -> Result<DxtAlgorithm, JsError> {
        match self.quality.as_str() {
            "fast" => Ok(DxtAlgorithm::RangeFit),
            "balanced" => Ok(DxtAlgorithm::ClusterFit),
            "best" => Ok(DxtAlgorithm::IterativeClusterFit),
            other => Err(JsError::new(&format!(
                "quality must be \"fast\", \"balanced\" or \"best\", got \"{other}\""
            ))),
        }
    }

    fn filter(&self) -> Result<FilterType, JsError> {
        match self.filter.as_str() {
            "nearest" => Ok(FilterType::Nearest),
            "triangle" => Ok(FilterType::Triangle),
            "catmullRom" => Ok(FilterType::CatmullRom),
            "gaussian" => Ok(FilterType::Gaussian),
            "lanczos3" => Ok(FilterType::Lanczos3),
            other => Err(JsError::new(&format!(
                "filter must be \"nearest\", \"triangle\", \"catmullRom\", \"gaussian\" or \"lanczos3\", got \"{other}\""
            ))),
        }
    }

    fn target(&self) -> Result<BlpTarget, JsError> {
        let old_format = |fmt: Option<&str>| -> Result<BlpOldFormat, JsError> {
            match fmt.unwrap_or("jpeg") {
                "raw1" => Ok(BlpOldFormat::Raw1 {
                    alpha_bits: self.alpha_bits()?,
                }),
                "jpeg" => Ok(BlpOldFormat::Jpeg {
                    has_alpha: self.has_alpha,
                }),
                other => Err(JsError::new(&format!(
                    "format \"{other}\" is not supported for blp1 (use \"raw1\" or \"jpeg\")"
                ))),
            }
        };
        let blp2_format = |fmt: Option<&str>| -> Result<Blp2Format, JsError> {
            match fmt.unwrap_or("dxt5") {
                "raw1" => Ok(Blp2Format::Raw1 {
                    alpha_bits: self.alpha_bits()?,
                }),
                "raw3" => Ok(Blp2Format::Raw3),
                "jpeg" => Ok(Blp2Format::Jpeg {
                    has_alpha: self.has_alpha,
                }),
                "dxt1" => Ok(Blp2Format::Dxt1 {
                    has_alpha: self.has_alpha,
                    compress_algorithm: self.dxt_algorithm()?,
                }),
                "dxt3" => Ok(Blp2Format::Dxt3 {
                    has_alpha: self.has_alpha,
                    compress_algorithm: self.dxt_algorithm()?,
                }),
                "dxt5" => Ok(Blp2Format::Dxt5 {
                    has_alpha: self.has_alpha,
                    compress_algorithm: self.dxt_algorithm()?,
                }),
                other => Err(JsError::new(&format!(
                    "unknown format \"{other}\" (use \"raw1\", \"raw3\", \"jpeg\", \"dxt1\", \"dxt3\" or \"dxt5\")"
                ))),
            }
        };
        let format = self.format.as_deref();
        match self.version.as_str() {
            "blp1" => Ok(BlpTarget::Blp1(old_format(format)?)),
            "blp2" => Ok(BlpTarget::Blp2(blp2_format(format)?)),
            other => Err(JsError::new(&format!(
                "version must be \"blp1\" or \"blp2\", got \"{other}\" (blp0 uses external mipmap files and is not supported)"
            ))),
        }
    }
}

fn parse_options(options: Option<JsValue>) -> Result<EncodeOptions, JsError> {
    match options {
        None => Ok(EncodeOptions::default()),
        Some(value) if value.is_undefined() || value.is_null() => Ok(EncodeOptions::default()),
        Some(value) => serde_wasm_bindgen::from_value(value)
            .map_err(|e| JsError::new(&format!("invalid options object: {e}"))),
    }
}

fn encode_rgba(
    rgba: &[u8],
    width: u32,
    height: u32,
    options: Option<JsValue>,
) -> Result<Vec<u8>, JsError> {
    if rgba.len() != (width as usize) * (height as usize) * 4 {
        return Err(JsError::new(&format!(
            "rgba length mismatch: expected width*height*4 = {} bytes, got {}",
            width as usize * height as usize * 4,
            rgba.len()
        )));
    }
    let options = parse_options(options)?;
    let target = options.target()?;
    let filter = options.filter()?;

    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| JsError::new("invalid RGBA buffer dimensions"))?;
    let blp = image_to_blp(
        image::DynamicImage::ImageRgba8(img),
        options.mipmaps,
        target,
        filter,
    )
    .map_err(|e| JsError::new(&format!("failed to convert image to BLP: {e}")))?;
    encode_blp(&blp).map_err(|e| JsError::new(&format!("failed to encode BLP: {e}")))
}

/// Encode raw RGBA pixels into a BLP texture.
///
/// `rgba` must be `width * height * 4` bytes. `options` is an optional
/// object, see the README for all fields:
/// `{ version, format, alphaBits, hasAlpha, quality, mipmaps, filter }`.
#[wasm_bindgen(js_name = encodeBlp)]
pub fn encode_blp_js(
    rgba: &[u8],
    width: u32,
    height: u32,
    options: Option<JsValue>,
) -> Result<js_sys::Uint8Array, JsError> {
    let bytes = encode_rgba(rgba, width, height, options)?;
    Ok(to_uint8_array(&bytes))
}

/// Decode a BLP texture and return it as PNG bytes (main mipmap level, or
/// the given `mipmapLevel`).
#[wasm_bindgen(js_name = blpToPng)]
pub fn blp_to_png(data: &[u8], mipmap_level: Option<u32>) -> Result<js_sys::Uint8Array, JsError> {
    let image = parse(data)?;
    let (rgba, _, _) = decode_level(&image, mipmap_level)?;
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| JsError::new(&format!("failed to encode PNG: {e}")))?;
    Ok(to_uint8_array(&png.into_inner()))
}

/// Decode PNG (or any common image format: JPEG, BMP, TGA, …) bytes and
/// encode them as a BLP texture. Accepts the same options as
/// [`encodeBlp`](encode_blp_js).
#[wasm_bindgen(js_name = pngToBlp)]
pub fn png_to_blp(png: &[u8], options: Option<JsValue>) -> Result<js_sys::Uint8Array, JsError> {
    let img = image::load_from_memory(png)
        .map_err(|e| JsError::new(&format!("failed to decode image: {e}")))?
        .to_rgba8();
    let (w, h) = img.dimensions();
    let bytes = encode_rgba(img.as_raw(), w, h, options)?;
    Ok(to_uint8_array(&bytes))
}
