//! EXIF Extraction & Auto-Orientation Helper
//!
//! Uses `kamadak-exif` and a built-in pure-Rust TIFF/MakerNote parser to extract
//! rich shooting metadata, camera actuations / shutter count, lens details, and
//! orientation flags for photographic files and Camera RAW formats.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct RawSensorInfo {
    pub clean_make: String,
    pub clean_model: String,
    pub sensor_width: usize,
    pub sensor_height: usize,
    pub active_width: usize,
    pub active_height: usize,
    pub cfa_pattern: String,
    pub bit_depth_est: String,
    pub black_level: String,
    pub white_level: String,
    pub wb_multipliers: String,
    pub raw_orientation: u32,
}

#[derive(Debug, Clone, Default)]
pub struct ExifMetadata {
    pub orientation: u32,

    // Camera & Actuations
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub body_serial: Option<String>,
    pub internal_serial: Option<String>,
    pub firmware: Option<String>,
    pub shutter_count: Option<String>,
    pub image_number: Option<String>,
    pub drive_mode: Option<String>,
    pub focus_mode: Option<String>,
    pub af_system: Option<String>,
    pub camera_temp: Option<String>,

    // Exposure & Metering
    pub shutter_speed: Option<String>,
    pub aperture: Option<String>,
    pub iso: Option<String>,
    pub exposure_program: Option<String>,
    pub exposure_mode: Option<String>,
    pub exposure_bias: Option<String>,
    pub metering_mode: Option<String>,
    pub white_balance: Option<String>,
    pub light_source: Option<String>,
    pub color_temp: Option<String>,
    pub picture_style: Option<String>,
    pub flash: Option<String>,

    // Optics & Focus
    pub lens_make: Option<String>,
    pub lens_model: Option<String>,
    pub lens_spec: Option<String>,
    pub lens_serial: Option<String>,
    pub focal_length: Option<String>,
    pub focal_length_35mm: Option<String>,
    pub max_aperture: Option<String>,
    pub subject_distance: Option<String>,

    // Author, GPS & File
    pub date_taken: Option<String>,
    pub date_digitized: Option<String>,
    pub artist: Option<String>,
    pub copyright: Option<String>,
    pub color_space: Option<String>,
    pub gps_coords: Option<String>,
    pub gps_altitude: Option<String>,

    // Populated by rawloader for Camera RAW files
    pub raw_sensor: Option<RawSensorInfo>,
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

    // Helper to find a field in PRIMARY IFD or any available IFD
    let find_field = |tag| {
        exif.get_field(tag, exif::In::PRIMARY)
            .or_else(|| exif.fields().find(|f| f.tag == tag))
    };

    let get_str = |tag| {
        find_field(tag)
            .map(|f| f.display_value().to_string().trim_matches('"').trim().to_string())
            .filter(|s| !s.is_empty())
    };

    // 1. Orientation
    if let Some(field) = find_field(exif::Tag::Orientation) {
        if let Some(val) = field.value.get_uint(0) {
            meta.orientation = val;
        }
    }

    // 2. Camera Make, Model & Body
    meta.camera_make = get_str(exif::Tag::Make);
    meta.camera_model = get_str(exif::Tag::Model);
    meta.body_serial = get_str(exif::Tag::BodySerialNumber);
    meta.firmware = get_str(exif::Tag::Software);

    // Standard ImageNumber (TIFF/EP Tag 0x9211)
    if let Some(num) = get_str(exif::Tag(exif::Context::Tiff, 0x9211)) {
        meta.image_number = Some(num);
    }

    // 3. Exposure & Metering
    if let Some(f) = find_field(exif::Tag::ExposureTime) {
        meta.shutter_speed = Some(format!("{} s", f.display_value().to_string().trim_end_matches(" s")));
    } else if let Some(f) = find_field(exif::Tag::ShutterSpeedValue) {
        meta.shutter_speed = Some(f.display_value().to_string());
    }

    if let Some(f) = find_field(exif::Tag::FNumber) {
        meta.aperture = Some(format!("f/{}", f.display_value()));
    } else if let Some(f) = find_field(exif::Tag::ApertureValue) {
        meta.aperture = Some(format!("f/{}", f.display_value()));
    }

    if let Some(f) = find_field(exif::Tag::PhotographicSensitivity)
        .or_else(|| find_field(exif::Tag::RecommendedExposureIndex))
        .or_else(|| find_field(exif::Tag::StandardOutputSensitivity))
    {
        meta.iso = Some(format!("ISO {}", f.display_value()));
    }

    meta.exposure_program = get_str(exif::Tag::ExposureProgram);
    meta.exposure_mode = get_str(exif::Tag::ExposureMode);

    if let Some(f) = find_field(exif::Tag::ExposureBiasValue) {
        let val_str = f.display_value().to_string();
        let trimmed = val_str.trim();
        if trimmed == "0" || trimmed == "0/1" {
            meta.exposure_bias = Some("0.0 EV".into());
        } else if trimmed.starts_with('+') || trimmed.starts_with('-') {
            meta.exposure_bias = Some(format!("{} EV", trimmed));
        } else {
            meta.exposure_bias = Some(format!("+{} EV", trimmed));
        }
    }

    meta.metering_mode = get_str(exif::Tag::MeteringMode);
    meta.white_balance = get_str(exif::Tag::WhiteBalance);
    meta.light_source = get_str(exif::Tag::LightSource);
    meta.flash = get_str(exif::Tag::Flash);

    // 4. Optics & Lens
    meta.lens_make = get_str(exif::Tag::LensMake);
    meta.lens_model = get_str(exif::Tag::LensModel)
        .or_else(|| get_str(exif::Tag(exif::Context::Tiff, 0xA434))); // EXIF Tag 0xA434 = LensModel
    meta.lens_spec = get_str(exif::Tag::LensSpecification)
        .or_else(|| get_str(exif::Tag(exif::Context::Tiff, 0xA432)));
    meta.lens_serial = get_str(exif::Tag::LensSerialNumber)
        .or_else(|| get_str(exif::Tag(exif::Context::Tiff, 0xA435)));

    if let Some(f) = find_field(exif::Tag::FocalLength) {
        meta.focal_length = Some(format!("{} mm", f.display_value().to_string().trim_end_matches(" mm")));
    }
    if let Some(f) = find_field(exif::Tag::FocalLengthIn35mmFilm) {
        meta.focal_length_35mm = Some(format!("{} mm (35mm eq)", f.display_value().to_string().trim_end_matches(" mm")));
    }
    if let Some(f) = find_field(exif::Tag::MaxApertureValue) {
        meta.max_aperture = Some(format!("f/{}", f.display_value()));
    }
    if let Some(f) = find_field(exif::Tag::SubjectDistance) {
        let d = f.display_value().to_string();
        meta.subject_distance = Some(format!("{} m", d.trim_end_matches(" m")));
    }

    // If 35mm equivalent focal length was not explicitly written, calculate it from sensor focal plane resolution
    if meta.focal_length_35mm.is_none() {
        if let (Some(f_mm_val), Some(fx_res_field), Some(w_field)) = (
            find_field(exif::Tag::FocalLength).and_then(|f| match f.value {
                exif::Value::Rational(ref r) if !r.is_empty() && r[0].denom != 0 => {
                    Some(r[0].num as f32 / r[0].denom as f32)
                }
                _ => None,
            }),
            find_field(exif::Tag(exif::Context::Tiff, 0xA20E)).and_then(|f| match f.value {
                exif::Value::Rational(ref r) if !r.is_empty() && r[0].denom != 0 => {
                    Some(r[0].num as f32 / r[0].denom as f32)
                }
                _ => None,
            }),
            find_field(exif::Tag(exif::Context::Tiff, 0xA002))
                .or_else(|| find_field(exif::Tag(exif::Context::Tiff, 0x0100)))
                .and_then(|f| f.value.get_uint(0)),
        ) {
            if fx_res_field > 0.0 {
                let sensor_w_mm = (w_field as f32 / fx_res_field) * 25.4;
                if sensor_w_mm > 5.0 && sensor_w_mm < 50.0 {
                    let crop_factor = 36.0 / sensor_w_mm;
                    let eq_35 = (f_mm_val * crop_factor).round() as u32;
                    meta.focal_length_35mm = Some(format!("{} mm ({:.1}x eq)", eq_35, crop_factor));
                }
            }
        }
    }

    // 5. Author, GPS & Date
    if let Some(dt) = get_str(exif::Tag::DateTimeOriginal) {
        let subsec = get_str(exif::Tag::SubSecTimeOriginal).unwrap_or_default();
        let offset = get_str(exif::Tag::OffsetTimeOriginal).unwrap_or_default();
        let mut full_dt = dt;
        if !subsec.is_empty() {
            full_dt.push('.');
            full_dt.push_str(&subsec);
        }
        if !offset.is_empty() {
            full_dt.push(' ');
            full_dt.push_str(&offset);
        }
        meta.date_taken = Some(full_dt);
    }
    meta.date_digitized = get_str(exif::Tag::DateTimeDigitized);

    meta.artist = get_str(exif::Tag::CameraOwnerName)
        .or_else(|| get_str(exif::Tag::Artist))
        .or_else(|| get_str(exif::Tag(exif::Context::Tiff, 0x013B))); // Artist tag in IFD0
    meta.copyright = get_str(exif::Tag::Copyright);
    meta.color_space = get_str(exif::Tag::ColorSpace);

    if let (Some(lat), Some(lon)) = (find_field(exif::Tag::GPSLatitude), find_field(exif::Tag::GPSLongitude)) {
        let lat_ref = get_str(exif::Tag::GPSLatitudeRef).unwrap_or_default();
        let lon_ref = get_str(exif::Tag::GPSLongitudeRef).unwrap_or_default();
        meta.gps_coords = Some(
            format!("{} {}, {} {}", lat.display_value(), lat_ref, lon.display_value(), lon_ref)
                .trim()
                .to_string(),
        );
    }
    if let Some(alt) = find_field(exif::Tag::GPSAltitude) {
        meta.gps_altitude = Some(format!("{} m", alt.display_value().to_string().trim_end_matches(" m")));
    }

    // 6. Camera MakerNotes (Nikon, Canon, Pentax, Sony)
    if let Some(f) = find_field(exif::Tag::MakerNote) {
        let (raw_bytes, base_offset) = match f.value {
            exif::Value::Undefined(ref bytes, offset) => (Some(bytes.as_slice()), offset as usize),
            exif::Value::Byte(ref bytes) => (Some(bytes.as_slice()), 0),
            _ => (None, 0),
        };

        if let Some(bytes) = raw_bytes {
            let mn = parse_makernote(bytes, base_offset);
            if let Some(sc) = mn.shutter_count {
                meta.shutter_count = Some(format_number(sc));
            }
            if meta.image_number.is_none() {
                meta.image_number = mn.file_number;
            }
            if meta.body_serial.is_none() {
                meta.body_serial = mn.serial_number;
            }
            meta.internal_serial = mn.internal_serial;
            if meta.firmware.is_none() {
                meta.firmware = mn.firmware;
            }
            if meta.artist.is_none() {
                meta.artist = mn.owner_name;
            }
            if meta.lens_model.is_none() {
                meta.lens_model = mn.lens_model;
            }
            if meta.camera_model.is_none() {
                meta.camera_model = mn.camera_model;
            }
            if meta.drive_mode.is_none() {
                meta.drive_mode = mn.drive_mode;
            }
            if meta.focus_mode.is_none() {
                meta.focus_mode = mn.focus_mode;
            }
            meta.af_system = mn.af_system;
            meta.camera_temp = mn.camera_temp;
            meta.color_temp = mn.color_temp;
            meta.picture_style = mn.picture_style;
            if meta.subject_distance.is_none() {
                meta.subject_distance = mn.subject_distance;
            }
        }
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

// ============================================================================
// Safe Pure-Rust MakerNote Parser (Nikon, Canon, Pentax, Sony)
// ============================================================================

#[derive(Default, Debug)]
struct MakerNoteInfo {
    shutter_count: Option<u32>,
    file_number: Option<String>,
    serial_number: Option<String>,
    internal_serial: Option<String>,
    firmware: Option<String>,
    owner_name: Option<String>,
    lens_model: Option<String>,
    camera_model: Option<String>,
    drive_mode: Option<String>,
    focus_mode: Option<String>,
    af_system: Option<String>,
    camera_temp: Option<String>,
    color_temp: Option<String>,
    picture_style: Option<String>,
    subject_distance: Option<String>,
}

fn read_u16(buf: &[u8], offset: usize, is_le: bool) -> Option<u16> {
    if offset + 2 <= buf.len() {
        let bytes = [buf[offset], buf[offset + 1]];
        Some(if is_le { u16::from_le_bytes(bytes) } else { u16::from_be_bytes(bytes) })
    } else {
        None
    }
}

fn read_i16(buf: &[u8], offset: usize, is_le: bool) -> Option<i16> {
    if offset + 2 <= buf.len() {
        let bytes = [buf[offset], buf[offset + 1]];
        Some(if is_le { i16::from_le_bytes(bytes) } else { i16::from_be_bytes(bytes) })
    } else {
        None
    }
}

fn read_u32(buf: &[u8], offset: usize, is_le: bool) -> Option<u32> {
    if offset + 4 <= buf.len() {
        let bytes = [buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]];
        Some(if is_le { u32::from_le_bytes(bytes) } else { u32::from_be_bytes(bytes) })
    } else {
        None
    }
}

fn read_ascii_null_terminated(buf: &[u8], offset: usize, max_len: usize) -> Option<String> {
    if offset >= buf.len() {
        return None;
    }
    let end = (offset + max_len).min(buf.len());
    let slice = &buf[offset..end];
    let actual_slice = match slice.iter().position(|&b| b == 0) {
        Some(pos) => &slice[..pos],
        None => slice,
    };
    let s = String::from_utf8_lossy(actual_slice).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn format_number(val: u32) -> String {
    let s = val.to_string();
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    for (i, ch) in chars.into_iter().enumerate() {
        out.push(ch);
        let rem = len - 1 - i;
        if rem > 0 && rem % 3 == 0 {
            out.push(',');
        }
    }
    out
}

fn parse_makernote(data: &[u8], base_offset: usize) -> MakerNoteInfo {
    let mut info = MakerNoteInfo::default();
    if data.len() < 12 {
        return info;
    }

    // Helper to resolve an offset that may be absolute within the TIFF file or relative to the MakerNote
    let resolve_offset = |raw_offset: u32| -> Option<usize> {
        let raw = raw_offset as usize;
        if raw >= base_offset && raw < base_offset + data.len() {
            Some(raw - base_offset)
        } else if raw < data.len() {
            Some(raw)
        } else {
            None
        }
    };

    // 1. Nikon MakerNote
    if data.starts_with(b"Nikon\0\x02") || data.starts_with(b"Nikon\0\x01") {
        let mini = &data[10..];
        if mini.len() >= 8 {
            let is_le = &mini[..2] == b"II";
            if let Some(ifd_off) = read_u32(mini, 4, is_le) {
                let off = ifd_off as usize;
                if let Some(count) = read_u16(mini, off, is_le) {
                    for i in 0..count as usize {
                        let e = off + 2 + i * 12;
                        if let (Some(tag), Some(_type), Some(cnt), Some(val)) = (
                            read_u16(mini, e, is_le),
                            read_u16(mini, e + 2, is_le),
                            read_u32(mini, e + 4, is_le),
                            read_u32(mini, e + 8, is_le),
                        ) {
                            match tag {
                                0x00A7 => info.shutter_count = Some(val),
                                0x001D => {
                                    if let Some(s) = read_ascii_null_terminated(mini, val as usize, cnt as usize) {
                                        info.serial_number = Some(s);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        return info;
    }

    // 2. Pentax MakerNote (tag 0x005D = ShutterCount)
    if data.starts_with(b"PENTAX \0") {
        let mini = &data[8..];
        let is_le = if mini.len() >= 2 { &mini[..2] == b"II" } else { true };
        let start_off = if mini.len() >= 8 && (&mini[..2] == b"II" || &mini[..2] == b"MM") { 8 } else { 0 };
        if let Some(count) = read_u16(mini, start_off, is_le) {
            for i in 0..count as usize {
                let e = start_off + 2 + i * 12;
                if let (Some(tag), Some(val)) = (read_u16(mini, e, is_le), read_u32(mini, e + 8, is_le)) {
                    if tag == 0x005D {
                        info.shutter_count = Some(val);
                    }
                }
            }
        }
        return info;
    }

    // 3. Canon MakerNote
    // In Canon RAW (.CR2/.CR3), MakerNote starts with num_entries (u16) directly in TIFF endian (usually Little Endian)
    for is_le in [true, false] {
        if let Some(count) = read_u16(data, 0, is_le) {
            if count >= 1 && count <= 250 && 2 + (count as usize) * 12 <= data.len() {
                for i in 0..count as usize {
                    let e = 2 + i * 12;
                    let tag = match read_u16(data, e, is_le) { Some(t) => t, None => continue };
                    let val_type = read_u16(data, e + 2, is_le).unwrap_or(0);
                    let count_items = read_u32(data, e + 4, is_le).unwrap_or(0);
                    let val_offset = read_u32(data, e + 8, is_le).unwrap_or(0);

                    match tag {
                        0x0006 => { // CanonImageType
                            if let Some(off) = resolve_offset(val_offset) {
                                info.camera_model = read_ascii_null_terminated(data, off, count_items as usize);
                            }
                        }
                        0x0007 => { // FirmwareVersion
                            if let Some(off) = resolve_offset(val_offset) {
                                info.firmware = read_ascii_null_terminated(data, off, count_items as usize);
                            }
                        }
                        0x0008 => { // FileNumber
                            if val_offset > 0 {
                                let folder = val_offset >> 16;
                                let file = val_offset & 0xFFFF;
                                if folder > 0 && file > 0 {
                                    info.file_number = Some(format!("{:03}-{:04}", folder, file));
                                } else {
                                    info.file_number = Some(format!("{}", val_offset));
                                }
                            }
                        }
                        0x0009 => { // OwnerName
                            if let Some(off) = resolve_offset(val_offset) {
                                info.owner_name = read_ascii_null_terminated(data, off, count_items as usize);
                            }
                        }
                        0x000C => { // SerialNumber
                            if val_type == 2 {
                                if let Some(off) = resolve_offset(val_offset) {
                                    info.serial_number = read_ascii_null_terminated(data, off, count_items as usize);
                                }
                            } else if val_offset > 0 {
                                info.serial_number = Some(format!("{}", val_offset));
                            }
                        }
                        0x0093 => { // CanonFileInfo (Focus distance & potential ShutterCount)
                            if let Some(off) = resolve_offset(val_offset) {
                                if off + 44 <= data.len() {
                                    // Focus distance upper (index 20) and lower (index 21) in cm
                                    let dist_upper = read_u16(data, off + 20 * 2, is_le).unwrap_or(0);
                                    let dist_lower = read_u16(data, off + 21 * 2, is_le).unwrap_or(0);
                                    if dist_lower > 0 && dist_upper > 0 && dist_upper >= dist_lower {
                                        info.subject_distance = Some(format!(
                                            "{:.2} - {:.2} m",
                                            dist_lower as f32 / 100.0,
                                            dist_upper as f32 / 100.0
                                        ));
                                    }
                                }
                                if off + 8 <= data.len() {
                                    if let Some(sc) = read_u32(data, off + 4, is_le) {
                                        if sc > 0 && sc < 2_000_000 {
                                            info.shutter_count = Some(sc);
                                        }
                                    }
                                }
                            }
                        }
                        0x0095 => { // LensModel
                            if let Some(off) = resolve_offset(val_offset) {
                                info.lens_model = read_ascii_null_terminated(data, off, count_items as usize);
                            }
                        }
                        0x0096 => { // InternalSerialNumber
                            if let Some(off) = resolve_offset(val_offset) {
                                info.internal_serial = read_ascii_null_terminated(data, off, count_items as usize);
                            }
                        }
                        0x0026 => { // AFInfo2
                            if let Some(off) = resolve_offset(val_offset) {
                                if off + 8 <= data.len() {
                                    let af_mode = read_u16(data, off + 2, is_le).unwrap_or(0);
                                    let num_pts = read_u16(data, off + 4, is_le).unwrap_or(0);
                                    if num_pts > 0 {
                                        let mode_name = match af_mode {
                                            1 => "Auto AF",
                                            2 => "Single-point AF",
                                            _ => "AF",
                                        };
                                        info.af_system = Some(format!("{} pts ({})", num_pts, mode_name));
                                    }
                                }
                            }
                        }
                        0x00A0 => { // ProcessingInfo (Color Temp & Picture Style)
                            if let Some(off) = resolve_offset(val_offset) {
                                if off + 22 <= data.len() {
                                    // Color Temp (index 9)
                                    if let Some(ct) = read_i16(data, off + 9 * 2, is_le) {
                                        if ct >= 2000 && ct <= 15000 {
                                            info.color_temp = Some(format!("{} K", ct));
                                        }
                                    }
                                    // Picture Style (index 10)
                                    if let Some(ps) = read_i16(data, off + 10 * 2, is_le) {
                                        info.picture_style = match ps {
                                            129 => Some("Standard".into()),
                                            130 => Some("Portrait".into()),
                                            131 => Some("Landscape".into()),
                                            132 => Some("Neutral".into()),
                                            133 => Some("Faithful".into()),
                                            134 => Some("Monochrome".into()),
                                            135 => Some("Auto".into()),
                                            _ => None,
                                        };
                                    }
                                }
                            }
                        }
                        0x0001 => { // CanonCameraSettings
                            if let Some(off) = resolve_offset(val_offset) {
                                if let Some(drive) = read_u16(data, off + 10, is_le) {
                                    info.drive_mode = match drive {
                                        0 => Some("Single-shot".into()),
                                        1 => Some("Continuous".into()),
                                        2 => Some("Movie".into()),
                                        3 => Some("Continuous Speed Priority".into()),
                                        4 => Some("Continuous Low".into()),
                                        5 => Some("Continuous High".into()),
                                        6 => Some("Silent Single".into()),
                                        7 => Some("Silent Continuous".into()),
                                        _ => None,
                                    };
                                }
                                if let Some(focus) = read_u16(data, off + 14, is_le) {
                                    info.focus_mode = match focus {
                                        0 => Some("One-Shot AF".into()),
                                        1 => Some("AI Servo AF".into()),
                                        2 => Some("AI Focus AF".into()),
                                        3 => Some("Manual Focus".into()),
                                        _ => None,
                                    };
                                }
                            }
                        }
                        0x0004 => { // CanonShotInfo
                            if let Some(off) = resolve_offset(val_offset) {
                                // Camera Temperature is at index 12 (offset 24 bytes) in Canon ShotInfo
                                if let Some(temp) = read_i16(data, off + 24, is_le) {
                                    if temp > 128 && temp < 200 {
                                        info.camera_temp = Some(format!("{} °C", temp - 128));
                                    } else if temp > 0 && temp < 80 {
                                        info.camera_temp = Some(format!("{} °C", temp));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                break;
            }
        }
    }

    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(42), "42");
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(14250), "14,250");
        assert_eq!(format_number(1234567), "1,234,567");
    }

    #[test]
    fn test_nikon_makernote_shutter_count() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"Nikon\0\x02\0\0\0");
        buf.extend_from_slice(b"II");
        buf.extend_from_slice(&42u16.to_le_bytes());
        buf.extend_from_slice(&8u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&0x00A7u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&15420u32.to_le_bytes());

        let info = parse_makernote(&buf, 0);
        assert_eq!(info.shutter_count, Some(15420));
    }

    #[test]
    fn test_canon_makernote_file_number() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&0x0008u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&((100u32 << 16) | 240u32).to_le_bytes());

        let info = parse_makernote(&buf, 0);
        assert_eq!(info.file_number, Some("100-0240".to_string()));
    }

    #[test]
    fn test_real_canon_cr2_metadata() {
        let path = Path::new("/home/hetsumani/Pictures/Image_0060.CR2");
        if path.exists() {
            let meta = parse_exif(path);
            assert_eq!(meta.camera_make.as_deref(), Some("Canon"));
            assert_eq!(meta.camera_model.as_deref(), Some("Canon EOS REBEL T5"));
            assert_eq!(meta.lens_model.as_deref(), Some("EF-S18-55mm f/3.5-5.6 IS II"));
            assert_eq!(meta.body_serial.as_deref(), Some("162073045139"));
            assert_eq!(meta.internal_serial.as_deref(), Some("MA1872984"));
            assert_eq!(meta.camera_temp.as_deref(), Some("32 °C"));
            assert_eq!(meta.color_temp.as_deref(), Some("5200 K"));
            assert_eq!(meta.picture_style.as_deref(), Some("Auto"));
            assert_eq!(meta.af_system.as_deref(), Some("9 pts (Single-point AF)"));
            assert_eq!(meta.drive_mode.as_deref(), Some("Continuous"));
            assert_eq!(meta.focus_mode.as_deref(), Some("One-Shot AF"));
            assert_eq!(meta.subject_distance.as_deref(), Some("0.58 - 0.76 m"));
            assert_eq!(meta.artist.as_deref(), Some("Getsemani Oseguera"));
        }
    }
}
