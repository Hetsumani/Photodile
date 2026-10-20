//! EXIF Extraction & Auto-Orientation Helper
//!
//! Uses `kamadak-exif` in pure Rust to read camera shooting metadata and
//! orientation flags for photographic files.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ExifMetadata {
    pub orientation: u32,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens_model: Option<String>,
    pub focal_length: Option<String>,
    pub aperture: Option<String>,
    pub shutter_speed: Option<String>,
    pub iso: Option<String>,
    pub date_taken: Option<String>,
}

impl Default for ExifMetadata {
    fn default() -> Self {
        Self {
            orientation: 1, // 1 = Normal orientation
            camera_make: None,
            camera_model: None,
            lens_model: None,
            focal_length: None,
            aperture: None,
            shutter_speed: None,
            iso: None,
            date_taken: None,
        }
    }
}

pub fn parse_exif(path: &Path) -> ExifMetadata {
    let mut meta = ExifMetadata::default();

    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return meta,
    };

    let mut buf_reader = BufReader::new(file);
    let exif_reader = exif::Reader::new();
    let exif = match exif_reader.read_from_container(&mut buf_reader) {
        Ok(e) => e,
        Err(_) => return meta,
    };

    // Orientation
    if let Some(field) = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY) {
        if let Some(val) = field.value.get_uint(0) {
            meta.orientation = val;
        }
    }

    // Camera Make & Model
    if let Some(field) = exif.get_field(exif::Tag::Make, exif::In::PRIMARY) {
        meta.camera_make = Some(field.display_value().to_string().trim_matches('"').to_string());
    }
    if let Some(field) = exif.get_field(exif::Tag::Model, exif::In::PRIMARY) {
        meta.camera_model = Some(field.display_value().to_string().trim_matches('"').to_string());
    }

    // Lens Model
    if let Some(field) = exif.get_field(exif::Tag::LensModel, exif::In::PRIMARY) {
        meta.lens_model = Some(field.display_value().to_string().trim_matches('"').to_string());
    }

    // Focal Length
    if let Some(field) = exif.get_field(exif::Tag::FocalLength, exif::In::PRIMARY) {
        let disp = field.display_value().to_string();
        meta.focal_length = Some(format!("{} mm", disp.trim_end_matches(" mm")));
    }

    // Aperture (FNumber)
    if let Some(field) = exif.get_field(exif::Tag::FNumber, exif::In::PRIMARY) {
        meta.aperture = Some(format!("f/{}", field.display_value()));
    }

    // Shutter Speed (ExposureTime)
    if let Some(field) = exif.get_field(exif::Tag::ExposureTime, exif::In::PRIMARY) {
        meta.shutter_speed = Some(format!("{} s", field.display_value().to_string().trim_end_matches(" s")));
    }

    // ISO
    if let Some(field) = exif.get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY) {
        meta.iso = Some(format!("ISO {}", field.display_value()));
    }

    // Date Taken
    if let Some(field) = exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY) {
        meta.date_taken = Some(field.display_value().to_string().trim_matches('"').to_string());
    }

    meta
}

/// Applies EXIF orientation transforms to a `DynamicImage`.
pub fn apply_exif_orientation(img: image::DynamicImage, orientation: u32) -> image::DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img, // 1 or unknown: untouched
    }
}

