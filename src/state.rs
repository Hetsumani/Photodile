//! Application State & Session Controller
//!
//! Coordinates file scanning, image loading, filmstrip thumbnail generation,
//! and synchronization with the Slint UI.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::formats::{decode_photo, is_supported_image, make_thumbnail_buffer, to_slint_image, LoadedPhoto};
use crate::{AppWindow, ExifEntry, FilmstripItem};
use crate::viewport::Viewport;

pub struct AppState {
    pub session_files: Vec<PathBuf>,
    pub active_index: Option<usize>,
    pub current_photo: Option<LoadedPhoto>,
    pub viewport: Viewport,
    pub generation_id: Arc<AtomicUsize>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            session_files: Vec::new(),
            active_index: None,
            current_photo: None,
            viewport: Viewport::new(),
            generation_id: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Scans a directory for top-level supported image files and sorts them naturally.
    pub fn scan_directory(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && is_supported_image(&path) {
                    files.push(path);
                }
            }
        }
        // Natural sort (e.g. img1, img2, img10 instead of img1, img10, img2)
        files.sort_by(|a, b| {
            let name_a = a.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let name_b = b.file_name().and_then(|s| s.to_str()).unwrap_or("");
            natord::compare(name_a, name_b)
        });
        files
    }

    /// Loads dropped or selected file/folder paths into the session.
    pub fn load_paths(&mut self, ui: &AppWindow, raw_paths: Vec<PathBuf>, append: bool) {
        let mut new_files = Vec::new();

        for path in raw_paths {
            if path.is_dir() {
                let mut dir_files = Self::scan_directory(&path);
                new_files.append(&mut dir_files);
            } else if path.is_file() && is_supported_image(&path) {
                new_files.push(path);
            }
        }

        if new_files.is_empty() {
            return;
        }

        let start_index = if append {
            let existing_set: HashSet<_> = self.session_files.iter().cloned().collect();
            let added_index = self.session_files.len();
            for file in new_files {
                if !existing_set.contains(&file) {
                    self.session_files.push(file);
                }
            }
            added_index
        } else {
            self.session_files = new_files;
            0
        };

        if self.session_files.is_empty() {
            return;
        }

        // Show filmstrip automatically if more than 1 image is loaded
        let show_filmstrip = self.session_files.len() > 1;
        ui.set_show_filmstrip(show_filmstrip);

        // Populate initial filmstrip model (with placeholders for thumbnails)
        let filmstrip_items: Vec<FilmstripItem> = self
            .session_files
            .iter()
            .enumerate()
            .map(|(idx, path)| {
                let filename = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("image")
                    .to_string();
                FilmstripItem {
                    index: idx as i32,
                    filename: filename.into(),
                    thumbnail: slint::Image::default(),
                    has_thumbnail: false,
                }
            })
            .collect();

        let model = Rc::new(VecModel::from(filmstrip_items));
        ui.set_filmstrip_items(ModelRc::from(model.clone()));

        // Select initial photo
        let target_idx = start_index.min(self.session_files.len() - 1);
        self.select_index(ui, target_idx);

        // Spawn async background thumbnail worker pool
        self.spawn_thumbnail_generation(ui.as_weak(), self.session_files.clone());
    }

    /// Selects an image by index and loads it into the viewer.
    pub fn select_index(&mut self, ui: &AppWindow, index: usize) {
        if index >= self.session_files.len() {
            return;
        }

        self.active_index = Some(index);
        ui.set_active_index(index as i32);

        let path = &self.session_files[index];
        match decode_photo(path) {
            Ok(photo) => {
                let (w, h) = (photo.image.width() as f32, photo.image.height() as f32);
                let slint_img = to_slint_image(&photo.image);

                // Update UI state
                ui.set_has_image(true);
                ui.set_current_image(slint_img);
                ui.set_current_filename(
                    path.file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .into(),
                );
                ui.set_current_dimensions(format!("{} × {} px", w as u32, h as u32).into());
                ui.set_current_format(photo.format_name.clone().into());
                ui.set_current_file_size(format_file_size(photo.file_size_bytes).into());
                ui.set_current_index_text(
                    format!("{} / {}", index + 1, self.session_files.len()).into(),
                );

                // Populate EXIF sidebar entries
                let mut exif_entries = Vec::new();
                if let Some(ref make) = photo.exif.camera_make {
                    exif_entries.push(ExifEntry { label: "Make".into(), value: make.clone().into() });
                }
                if let Some(ref model) = photo.exif.camera_model {
                    exif_entries.push(ExifEntry { label: "Camera".into(), value: model.clone().into() });
                }
                if let Some(ref lens) = photo.exif.lens_model {
                    exif_entries.push(ExifEntry { label: "Lens".into(), value: lens.clone().into() });
                }
                if let Some(ref fl) = photo.exif.focal_length {
                    exif_entries.push(ExifEntry { label: "Focal Length".into(), value: fl.clone().into() });
                }
                if let Some(ref ap) = photo.exif.aperture {
                    exif_entries.push(ExifEntry { label: "Aperture".into(), value: ap.clone().into() });
                }
                if let Some(ref ss) = photo.exif.shutter_speed {
                    exif_entries.push(ExifEntry { label: "Shutter".into(), value: ss.clone().into() });
                }
                if let Some(ref iso) = photo.exif.iso {
                    exif_entries.push(ExifEntry { label: "ISO".into(), value: iso.clone().into() });
                }
                if let Some(ref dt) = photo.exif.date_taken {
                    exif_entries.push(ExifEntry { label: "Date Taken".into(), value: dt.clone().into() });
                }
                exif_entries.push(ExifEntry {
                    label: "Megapixels".into(),
                    value: format!("{:.1} MP", (w * h) / 1_000_000.0).into(),
                });
                exif_entries.push(ExifEntry {
                    label: "File Size".into(),
                    value: format_file_size(photo.file_size_bytes).into(),
                });
                exif_entries.push(ExifEntry {
                    label: "Full Path".into(),
                    value: path.to_string_lossy().to_string().into(),
                });

                let exif_model = Rc::new(VecModel::from(exif_entries));
                ui.set_exif_entries(ModelRc::from(exif_model));

                // Configure viewport and fit image
                self.viewport.set_image_dimensions(w, h);
                self.sync_viewport_to_ui(ui);

                self.current_photo = Some(photo);
            }
            Err(err) => {
                eprintln!("Error decoding image {:?}: {:?}", path, err);
                ui.set_current_filename(format!("Error loading: {:?}", err).into());
            }
        }
    }

    pub fn navigate_prev(&mut self, ui: &AppWindow) {
        if let Some(idx) = self.active_index {
            if idx > 0 {
                self.select_index(ui, idx - 1);
            }
        }
    }

    pub fn navigate_next(&mut self, ui: &AppWindow) {
        if let Some(idx) = self.active_index {
            if idx + 1 < self.session_files.len() {
                self.select_index(ui, idx + 1);
            }
        }
    }

    pub fn sync_viewport_to_ui(&self, ui: &AppWindow) {
        ui.set_image_offset_x(self.viewport.offset_x);
        ui.set_image_offset_y(self.viewport.offset_y);
        ui.set_image_display_width(self.viewport.display_width());
        ui.set_image_display_height(self.viewport.display_height());
        ui.set_current_zoom(self.viewport.scale);
        ui.set_zoom_percentage_text(format!("{}%", self.viewport.zoom_percentage()).into());
    }

    /// Background asynchronous thumbnail generation worker pool.
    fn spawn_thumbnail_generation(
        &self,
        ui_weak: slint::Weak<AppWindow>,
        paths: Vec<PathBuf>,
    ) {
        let current_gen = self.generation_id.fetch_add(1, Ordering::SeqCst) + 1;
        let gen_tracker = self.generation_id.clone();

        std::thread::spawn(move || {
            for (idx, path) in paths.into_iter().enumerate() {
                // If a newer session has started, abort this batch
                if gen_tracker.load(Ordering::SeqCst) != current_gen {
                    return;
                }

                if let Ok(photo) = decode_photo(&path) {
                    let pixel_buffer = make_thumbnail_buffer(&photo.image);
                    let ui_weak = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            let slint_img = slint::Image::from_rgba8(pixel_buffer);
                            ui.invoke_set_thumbnail(idx as i32, slint_img);
                        }
                    });
                }
            }
        });
    }
}

fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

