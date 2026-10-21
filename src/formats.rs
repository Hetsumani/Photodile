//! Unified Multi-Format Image Codec Engine (100% Pure Rust)
//!
//! Handles decoding for Standard & Pro Raster (JPG, PNG, WebP, TIFF, EXR, HDR, BMP, GIF,
//! ICO, TGA, QOI, PNM, DDS, Farbfeld), Vector (SVG), Photoshop (PSD), Next-Gen (JXL),
//! and Camera RAW (CR2, NEF, ARW, DNG, ORF, RW2, PEF, RAF).
//! Also handles multi-format conversion and saving.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use image::{DynamicImage, GenericImageView, ImageEncoder, RgbaImage};

use crate::exif_meta::{apply_exif_orientation, parse_exif, ExifMetadata};

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    // Standard / Pro Raster
    "jpg", "jpeg", "jpe", "jfif", "png", "webp", "tif", "tiff",
    "exr", "hdr", "bmp", "dib", "gif", "ico", "tga", "qoi",
    "pbm", "pgm", "ppm", "pam", "pnm", "dds", "ff",
    // Vector
    "svg", "svgz",
    // Photoshop PSD
    "psd",
    // JPEG XL
    "jxl",
    // Camera RAW
    "dng", "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2",
    "orf", "rw2", "pef", "raf", "srw", "erf", "kdc", "dcr",
    "mos", "mef", "mrw",
];

pub fn is_supported_image(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        let ext_lower = ext.to_lowercase();
        SUPPORTED_EXTENSIONS.iter().any(|&e| e == ext_lower)
    } else {
        false
    }
}

pub struct LoadedPhoto {
    pub image: DynamicImage,
    pub format_name: String,
    pub exif: ExifMetadata,
    pub file_size_bytes: u64,
}

/// Decodes any supported photography format into a `DynamicImage` with EXIF orientation auto-applied.
pub fn decode_photo(path: &Path) -> Result<LoadedPhoto> {
    decode_photo_with_progress(path, |_, _| {})
}

/// Decodes any supported photography format into a `DynamicImage` with progress callbacks.
pub fn decode_photo_with_progress<F>(path: &Path, mut progress: F) -> Result<LoadedPhoto>
where
    F: FnMut(f32, &str),
{
    progress(0.05, "Reading file & EXIF metadata...");
    let file_size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let exif = parse_exif(path);
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    let (mut img, format_name) = match ext.as_str() {
        "svg" | "svgz" => {
            progress(0.20, "Parsing SVG vector data...");
            let data = std::fs::read(path).context("Failed to read SVG file")?;
            let opt = resvg::usvg::Options::default();
            let tree = resvg::usvg::Tree::from_data(&data, &opt)
                .map_err(|e| anyhow!("Failed to parse SVG: {:?}", e))?;
            
            let size = tree.size();
            let mut width = size.width().round() as u32;
            let mut height = size.height().round() as u32;
            if width == 0 || height == 0 {
                width = 800;
                height = 600;
            }

            // High-res rendering if too small for visualizer
            let scale_factor = if width < 1200 && height < 1200 { 2.0 } else { 1.0 };
            let render_w = (width as f32 * scale_factor).round() as u32;
            let render_h = (height as f32 * scale_factor).round() as u32;

            progress(0.50, "Rasterizing SVG canvas...");
            let mut pixmap = tiny_skia::Pixmap::new(render_w, render_h)
                .ok_or_else(|| anyhow!("Failed to allocate Pixmap for SVG"))?;
            
            let transform = tiny_skia::Transform::from_scale(scale_factor, scale_factor);
            resvg::render(&tree, transform, &mut pixmap.as_mut());

            let rgba = RgbaImage::from_raw(render_w, render_h, pixmap.take())
                .ok_or_else(|| anyhow!("Failed to construct RgbaImage from SVG pixmap"))?;
            (DynamicImage::ImageRgba8(rgba), "SVG".to_string())
        }

        "psd" => {
            progress(0.20, "Reading PSD data...");
            let data = std::fs::read(path).context("Failed to read PSD file")?;
            progress(0.50, "Parsing Photoshop layers & composite...");
            let psd = psd::Psd::from_bytes(&data)
                .map_err(|e| anyhow!("Failed to parse PSD: {:?}", e))?;
            let width = psd.width();
            let height = psd.height();
            let rgba_bytes = psd.rgba();
            let rgba = RgbaImage::from_raw(width, height, rgba_bytes)
                .ok_or_else(|| anyhow!("Failed to create RgbaImage from PSD"))?;
            (DynamicImage::ImageRgba8(rgba), "PSD".to_string())
        }

        "jxl" => {
            progress(0.20, "Opening JPEG XL image...");
            let jxl = jxl_oxide::JxlImage::builder()
                .open(path)
                .map_err(|e| anyhow!("Failed to open JXL: {:?}", e))?;
            progress(0.50, "Rendering JXL frame...");
            let render = jxl
                .render_frame(0)
                .map_err(|e| anyhow!("Failed to render JXL frame: {:?}", e))?;
            let fb = render.image_all_channels();
            let (w, h) = (fb.width() as u32, fb.height() as u32);
            let ch = fb.channels();
            let buf = fb.buf();
            let mut rgba = RgbaImage::new(w, h);
            for y in 0..h {
                for x in 0..w {
                    let idx = (y as usize * fb.width() + x as usize) * ch;
                    let r = (buf[idx].clamp(0.0, 1.0) * 255.0) as u8;
                    let g = (buf[idx + 1].clamp(0.0, 1.0) * 255.0) as u8;
                    let b = (buf[idx + 2].clamp(0.0, 1.0) * 255.0) as u8;
                    let a = if ch >= 4 { (buf[idx + 3].clamp(0.0, 1.0) * 255.0) as u8 } else { 255 };
                    rgba.put_pixel(x, y, image::Rgba([r, g, b, a]));
                }
            }
            (DynamicImage::ImageRgba8(rgba), "JPEG XL".to_string())
        }

        "dng" | "cr2" | "cr3" | "nef" | "nrw" | "arw" | "srf" | "sr2" | "orf" | "rw2" | "pef"
        | "raf" | "srw" | "erf" | "kdc" | "dcr" | "mos" | "mef" | "mrw" => {
            progress(0.20, "Decoding Camera RAW sensor data...");
            match decode_camera_raw_with_progress(path, &mut progress) {
                Ok(raw_img) => (raw_img, ext.to_uppercase()),
                Err(_) => {
                    progress(0.60, "Fallback: reading RAW preview stream...");
                    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
                    let fmt = reader.format().map(|f| format!("{:?}", f)).unwrap_or_else(|| ext.to_uppercase());
                    let decoded = reader.decode()?;
                    (decoded, fmt)
                }
            }
        }

        _ => {
            progress(0.25, "Decoding image stream...");
            let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
            // Relax memory limits for photography
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(65536);
            limits.max_image_height = Some(65536);
            limits.max_alloc = Some(1024 * 1024 * 1024); // 1GB alloc limit for huge photos
            reader.limits(limits);

            let fmt_name = reader
                .format()
                .map(|f| format!("{:?}", f))
                .unwrap_or_else(|| ext.to_uppercase());
            let decoded = reader.decode().context("Failed to decode image file")?;
            (decoded, fmt_name)
        }
    };

    // Auto-orient based on camera EXIF tags
    if exif.orientation > 1 {
        progress(0.85, "Applying EXIF orientation...");
        img = apply_exif_orientation(img, exif.orientation);
    }

    progress(0.95, "Finalizing image buffer...");

    Ok(LoadedPhoto {
        image: img,
        format_name,
        exif,
        file_size_bytes,
    })
}

fn decode_camera_raw_with_progress<F>(path: &Path, progress: &mut F) -> Result<DynamicImage>
where
    F: FnMut(f32, &str),
{
    progress(0.25, "Decoding Camera RAW sensor data...");
    let raw = rawloader::decode_file(path)
        .map_err(|e| anyhow!("Camera RAW decode error: {:?}", e))?;

    progress(0.50, "Developing sRGB color pipeline...");
    let mut pipeline = imagepipe::Pipeline::new_from_source(imagepipe::ImageSource::Raw(raw))
        .map_err(|e| anyhow!("imagepipe pipeline init error: {:?}", e))?;

    progress(0.70, "Demosaicing & generating 8-bit sRGB pixels...");
    let srgb_data = pipeline
        .output_8bit(None)
        .map_err(|e| anyhow!("imagepipe sRGB output error: {:?}", e))?;

    progress(0.85, "Constructing pixel buffer...");
    let rgb = image::RgbImage::from_raw(
        srgb_data.width as u32,
        srgb_data.height as u32,
        srgb_data.data,
    )
    .ok_or_else(|| anyhow!("Failed to build RgbImage from RAW data"))?;
    Ok(DynamicImage::ImageRgb8(rgb))
}

/// Converts a `DynamicImage` to a `slint::Image`
#[allow(dead_code)]
pub fn to_slint_image(img: &DynamicImage) -> slint::Image {
    let (buffer, _, _) = make_rgba_pixel_buffer(img);
    slint::Image::from_rgba8(buffer)
}

/// Builds a thread-safe `SharedPixelBuffer<Rgba8Pixel>` from a `DynamicImage`
pub fn make_rgba_pixel_buffer(img: &DynamicImage) -> (slint::SharedPixelBuffer<slint::Rgba8Pixel>, u32, u32) {
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let buffer = slint::SharedPixelBuffer::clone_from_slice(rgba.as_raw(), width, height);
    (buffer, width, height)
}

/// Generates a thread-safe thumbnail pixel buffer for the filmstrip
pub fn make_thumbnail_buffer(img: &DynamicImage) -> slint::SharedPixelBuffer<slint::Rgba8Pixel> {
    let thumb = img.thumbnail(160, 120);
    let rgba = thumb.to_rgba8();
    let (width, height) = rgba.dimensions();
    slint::SharedPixelBuffer::clone_from_slice(rgba.as_raw(), width, height)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Png,
    Jpeg,
    WebP,
    Avif,
    Tiff,
    Qoi,
    Bmp,
    OpenExr,
    Tga,
    Ico,
    Farbfeld,
    Pnm,
}

impl ExportFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Png => "png",
            ExportFormat::Jpeg => "jpg",
            ExportFormat::WebP => "webp",
            ExportFormat::Avif => "avif",
            ExportFormat::Tiff => "tif",
            ExportFormat::Qoi => "qoi",
            ExportFormat::Bmp => "bmp",
            ExportFormat::OpenExr => "exr",
            ExportFormat::Tga => "tga",
            ExportFormat::Ico => "ico",
            ExportFormat::Farbfeld => "ff",
            ExportFormat::Pnm => "pnm",
        }
    }

    pub fn from_index(index: i32) -> Self {
        match index {
            0 => ExportFormat::Png,
            1 => ExportFormat::Jpeg,
            2 => ExportFormat::WebP,
            3 => ExportFormat::Avif,
            4 => ExportFormat::Tiff,
            5 => ExportFormat::Qoi,
            6 => ExportFormat::Bmp,
            7 => ExportFormat::OpenExr,
            8 => ExportFormat::Tga,
            _ => ExportFormat::Png,
        }
    }

    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "png" => ExportFormat::Png,
            "jpg" | "jpeg" | "jpe" | "jfif" => ExportFormat::Jpeg,
            "webp" => ExportFormat::WebP,
            "avif" => ExportFormat::Avif,
            "tif" | "tiff" => ExportFormat::Tiff,
            "qoi" => ExportFormat::Qoi,
            "bmp" => ExportFormat::Bmp,
            "exr" => ExportFormat::OpenExr,
            "tga" => ExportFormat::Tga,
            "ico" => ExportFormat::Ico,
            "ff" => ExportFormat::Farbfeld,
            "pbm" | "pgm" | "ppm" | "pnm" => ExportFormat::Pnm,
            _ => ExportFormat::Png,
        }
    }
}

/// Saves or converts an image into the chosen destination format with optional quality.
pub fn save_or_convert_image(
    img: &DynamicImage,
    dest: &Path,
    format: ExportFormat,
    quality: u8,
) -> Result<()> {
    let file = File::create(dest)
        .with_context(|| format!("Failed to create destination file {:?}", dest))?;
    let mut writer = BufWriter::new(file);

    let (w, h) = img.dimensions();

    match format {
        ExportFormat::Png => {
            let rgba = img.to_rgba8();
            image::codecs::png::PngEncoder::new(&mut writer)
                .write_image(&rgba, w, h, image::ExtendedColorType::Rgba8)?;
        }
        ExportFormat::Jpeg => {
            let rgb = img.to_rgb8();
            let q = quality.clamp(1, 100);
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, q);
            encoder.write_image(&rgb, w, h, image::ExtendedColorType::Rgb8)?;
        }
        ExportFormat::WebP => {
            let rgba = img.to_rgba8();
            image::codecs::webp::WebPEncoder::new_lossless(&mut writer)
                .write_image(&rgba, w, h, image::ExtendedColorType::Rgba8)?;
        }
        ExportFormat::Avif => {
            let rgba = img.to_rgba8();
            let q = quality.clamp(1, 100);
            let encoder = image::codecs::avif::AvifEncoder::new_with_speed_quality(&mut writer, 6, q);
            encoder.write_image(&rgba, w, h, image::ExtendedColorType::Rgba8)?;
        }
        ExportFormat::Tiff => {
            let rgba = img.to_rgba8();
            image::codecs::tiff::TiffEncoder::new(&mut writer)
                .write_image(&rgba, w, h, image::ExtendedColorType::Rgba8)?;
        }
        ExportFormat::Qoi => {
            let rgba = img.to_rgba8();
            image::codecs::qoi::QoiEncoder::new(&mut writer)
                .write_image(&rgba, w, h, image::ExtendedColorType::Rgba8)?;
        }
        ExportFormat::Bmp => {
            let rgb = img.to_rgb8();
            image::codecs::bmp::BmpEncoder::new(&mut writer)
                .write_image(&rgb, w, h, image::ExtendedColorType::Rgb8)?;
        }
        ExportFormat::OpenExr => {
            let rgba = img.to_rgba32f();
            image::codecs::openexr::OpenExrEncoder::new(&mut writer)
                .write_image(bytemuck::cast_slice(rgba.as_raw()), w, h, image::ExtendedColorType::Rgba32F)?;
        }
        ExportFormat::Tga => {
            let rgba = img.to_rgba8();
            image::codecs::tga::TgaEncoder::new(&mut writer)
                .write_image(&rgba, w, h, image::ExtendedColorType::Rgba8)?;
        }
        ExportFormat::Ico => {
            // ICO has max size 256x256
            let ico_img = if w > 256 || h > 256 {
                img.thumbnail(256, 256)
            } else {
                img.clone()
            };
            let rgba = ico_img.to_rgba8();
            let (iw, ih) = rgba.dimensions();
            image::codecs::ico::IcoEncoder::new(&mut writer)
                .write_image(&rgba, iw, ih, image::ExtendedColorType::Rgba8)?;
        }
        ExportFormat::Farbfeld => {
            let rgba = img.to_rgba16();
            image::codecs::farbfeld::FarbfeldEncoder::new(&mut writer)
                .write_image(bytemuck::cast_slice(rgba.as_raw()), w, h, image::ExtendedColorType::Rgba16)?;
        }
        ExportFormat::Pnm => {
            let rgb = img.to_rgb8();
            image::codecs::pnm::PnmEncoder::new(&mut writer)
                .write_image(&rgb, w, h, image::ExtendedColorType::Rgb8)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn make_test_image(w: u32, h: u32) -> DynamicImage {
        let mut img = RgbaImage::new(w, h);
        for x in 0..w {
            for y in 0..h {
                img.put_pixel(x, y, Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]));
            }
        }
        DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn test_supported_extensions() {
        assert!(is_supported_image(Path::new("photo.jpg")));
        assert!(is_supported_image(Path::new("photo.JPEG")));
        assert!(is_supported_image(Path::new("vector.svg")));
        assert!(is_supported_image(Path::new("graphic.psd")));
        assert!(is_supported_image(Path::new("canon.cr2")));
        assert!(is_supported_image(Path::new("nikon.nef")));
        assert!(is_supported_image(Path::new("sony.arw")));
        assert!(!is_supported_image(Path::new("document.pdf")));
        assert!(!is_supported_image(Path::new("program.exe")));
    }

    #[test]
    fn test_export_and_decode_roundtrips() {
        let temp_dir = std::env::temp_dir().join("photodile_test");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let original = make_test_image(64, 48);

        let test_formats = [
            (ExportFormat::Png, "test.png"),
            (ExportFormat::Jpeg, "test.jpg"),
            (ExportFormat::WebP, "test.webp"),
            (ExportFormat::Bmp, "test.bmp"),
            (ExportFormat::Tga, "test.tga"),
            (ExportFormat::Qoi, "test.qoi"),
            (ExportFormat::Tiff, "test.tif"),
        ];

        for (fmt, filename) in test_formats {
            let target_path = temp_dir.join(filename);
            save_or_convert_image(&original, &target_path, fmt, 90).expect("Save failed");
            assert!(target_path.exists());

            let loaded = decode_photo(&target_path).expect("Decode failed");
            assert_eq!(loaded.image.width(), 64);
            assert_eq!(loaded.image.height(), 48);

            let _ = std::fs::remove_file(target_path);
        }

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_decode_photo_with_progress() {
        let temp_dir = std::env::temp_dir().join("photodile_progress_test");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let target_path = temp_dir.join("test_progress.png");

        let original = make_test_image(32, 32);
        save_or_convert_image(&original, &target_path, ExportFormat::Png, 90).unwrap();

        let mut reports = Vec::new();
        let loaded = decode_photo_with_progress(&target_path, |p, stage| {
            reports.push((p, stage.to_string()));
        }).expect("Failed to decode with progress");

        assert_eq!(loaded.image.width(), 32);
        assert!(!reports.is_empty());
        assert!(reports.first().unwrap().0 <= 0.1);
        assert!(reports.last().unwrap().0 >= 0.9);

        let _ = std::fs::remove_file(target_path);
        let _ = std::fs::remove_dir_all(temp_dir);
    }
}

