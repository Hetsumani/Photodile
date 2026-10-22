//! Application State & Session Controller
//!
//! Coordinates file scanning, image loading, filmstrip thumbnail generation,
//! and synchronization with the Slint UI.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::formats::{
    decode_photo, decode_photo_with_progress, is_supported_image, make_rgba_pixel_buffer,
    make_thumbnail_buffer, LoadedPhoto,
};
use crate::viewport::Viewport;
use crate::{AppWindow, ExifEntry, FilmstripItem};

#[derive(Clone)]
pub struct CachedPhoto {
    pub photo: Arc<LoadedPhoto>,
    pub rgba_buf: slint::SharedPixelBuffer<slint::Rgba8Pixel>,
    pub w: u32,
    pub h: u32,
    pub thumb_buf: slint::SharedPixelBuffer<slint::Rgba8Pixel>,
}

pub struct AppState {
    pub session_files: Vec<PathBuf>,
    pub active_index: Option<usize>,
    pub displayed_index: Option<usize>,
    pub current_photo: Option<LoadedPhoto>,
    pub viewport: Viewport,
    pub generation_id: Arc<AtomicUsize>,
    pub load_request_id: Arc<AtomicUsize>,
    pub is_loading_main: Arc<AtomicBool>,
    pub image_cache: HashMap<PathBuf, CachedPhoto>,
    pub preload_request_id: Arc<AtomicUsize>,
    pub transition_id: Arc<AtomicUsize>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            session_files: Vec::new(),
            active_index: None,
            displayed_index: None,
            current_photo: None,
            viewport: Viewport::new(),
            generation_id: Arc::new(AtomicUsize::new(0)),
            load_request_id: Arc::new(AtomicUsize::new(0)),
            is_loading_main: Arc::new(AtomicBool::new(false)),
            image_cache: HashMap::new(),
            preload_request_id: Arc::new(AtomicUsize::new(0)),
            transition_id: Arc::new(AtomicUsize::new(0)),
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
    pub fn load_paths(state: &Arc<Mutex<AppState>>, ui: &AppWindow, raw_paths: Vec<PathBuf>, append: bool) {
        let mut new_files = Vec::new();

        for path in raw_paths {
            let path = if let Some(path_str) = path.to_str() {
                if path_str.starts_with("file://") {
                    let stripped = &path_str[7..];
                    let decoded = urlencoding::decode(stripped)
                        .map(|d| d.into_owned())
                        .unwrap_or_else(|_| stripped.to_string());
                    PathBuf::from(decoded)
                } else {
                    path
                }
            } else {
                path
            };

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

        let (target_idx, session_files_clone) = {
            let mut s = state.lock().unwrap();

            let start_index = if append {
                let existing_set: HashSet<_> = s.session_files.iter().cloned().collect();
                let added_index = s.session_files.len();
                for file in new_files {
                    if !existing_set.contains(&file) {
                        s.session_files.push(file);
                    }
                }
                added_index
            } else {
                s.session_files = new_files;
                s.displayed_index = None;
                s.image_cache.clear();
                s.preload_request_id.fetch_add(1, Ordering::SeqCst);
                s.transition_id.fetch_add(1, Ordering::SeqCst);
                ui.set_has_prev_image(false);
                ui.set_crossfade_progress(1.0);
                0
            };

            if s.session_files.is_empty() {
                return;
            }

            // Show filmstrip automatically if more than 1 image is loaded
            let show_filmstrip = s.session_files.len() > 1;
            ui.set_show_filmstrip(show_filmstrip);

            // Populate initial filmstrip model (with placeholders for thumbnails)
            let filmstrip_items: Vec<FilmstripItem> = s
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

            // Select initial photo: keep existing if appending, otherwise select first new
            let target = if append && s.active_index.is_some() {
                s.active_index.unwrap().min(s.session_files.len() - 1)
            } else {
                start_index.min(s.session_files.len() - 1)
            };

            (target, s.session_files.clone())
        };

        // Select photo asynchronously with progress tracking
        Self::select_index(state, ui, target_idx);

        // Spawn async background thumbnail worker pool
        state.lock().unwrap().spawn_thumbnail_generation(ui.as_weak(), session_files_clone);
    }

    /// Preloads up to 4 neighbor images (+1, -1, +2, -2) around the current active photo in the background.
    pub fn preload_neighbors(state: &Arc<Mutex<AppState>>, current_idx: usize) {
        let (req_id, req_tracker, needed) = {
            let mut s = state.lock().unwrap();
            let total = s.session_files.len();
            if total <= 1 {
                return;
            }

            let req_id = s.preload_request_id.fetch_add(1, Ordering::SeqCst) + 1;

            // Neighbor candidate indices in priority order: +1, -1, +2, -2
            let mut candidates = Vec::new();
            if current_idx + 1 < total {
                candidates.push(current_idx + 1);
            }
            if current_idx > 0 {
                candidates.push(current_idx - 1);
            }
            if current_idx + 2 < total {
                candidates.push(current_idx + 2);
            }
            if current_idx >= 2 {
                candidates.push(current_idx - 2);
            }

            // Prune cache: retain only active photo and up to 4 neighbor candidate photos
            let keep_paths: HashSet<PathBuf> = candidates
                .iter()
                .chain(std::iter::once(&current_idx))
                .filter_map(|&idx| s.session_files.get(idx).cloned())
                .collect();

            s.image_cache.retain(|path, _| keep_paths.contains(path));

            // Identify which candidate paths are not yet cached
            let mut needed = Vec::new();
            for idx in candidates {
                if let Some(path) = s.session_files.get(idx) {
                    if !s.image_cache.contains_key(path) {
                        needed.push((idx, path.clone()));
                    }
                }
            }

            (req_id, s.preload_request_id.clone(), needed)
        };

        if needed.is_empty() {
            return;
        }

        let state_clone = state.clone();
        std::thread::spawn(move || {
            for (_idx, path) in needed {
                if req_tracker.load(Ordering::SeqCst) != req_id {
                    return;
                }

                if let Ok(photo) = decode_photo(&path) {
                    if req_tracker.load(Ordering::SeqCst) != req_id {
                        return;
                    }
                    let (rgba_buf, w, h) = make_rgba_pixel_buffer(&photo.image);
                    let thumb_buf = make_thumbnail_buffer(&photo.image);
                    let cached = CachedPhoto {
                        photo: Arc::new(photo),
                        rgba_buf,
                        w,
                        h,
                        thumb_buf,
                    };

                    let mut s = state_clone.lock().unwrap();
                    if req_tracker.load(Ordering::SeqCst) == req_id {
                        s.image_cache.insert(path, cached);
                    }
                }
            }
        });
    }

    /// Selects an image by index and loads it into the viewer asynchronously with progress reporting.
    pub fn select_index(state: &Arc<Mutex<AppState>>, ui: &AppWindow, index: usize) {
        let (path, filename, req_id, req_tracker, is_loading_main, cached_hit) = {
            let mut s = state.lock().unwrap();
            if index >= s.session_files.len() {
                return;
            }

            s.active_index = Some(index);
            ui.set_active_index(index as i32);

            let path = s.session_files[index].clone();
            let filename = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("photo")
                .to_string();

            let req_id = s.load_request_id.fetch_add(1, Ordering::SeqCst) + 1;
            let cached_hit = s.image_cache.get(&path).cloned();

            (
                path,
                filename,
                req_id,
                s.load_request_id.clone(),
                s.is_loading_main.clone(),
                cached_hit,
            )
        };

        // Cache fast-path: instant display and smooth transition without disk wait
        if let Some(cached) = cached_hit {
            is_loading_main.store(false, Ordering::SeqCst);
            ui.set_is_loading(false);
            {
                let mut s = state.lock().unwrap();
                s.apply_cached_photo(ui, index, &path, cached);
            }
            Self::preload_neighbors(state, index);
            return;
        }

        // Cache miss: decode asynchronously with progress reporting
        is_loading_main.store(true, Ordering::SeqCst);
        let ui_weak = ui.as_weak();
        let state_clone = state.clone();

        std::thread::spawn(move || {
            let start_time = Instant::now();
            let progress_shown = Arc::new(AtomicBool::new(false));

            let progress_shown_clone = progress_shown.clone();
            let ui_weak_progress = ui_weak.clone();
            let req_tracker_progress = req_tracker.clone();
            let fname = filename.clone();

            let progress_cb = move |pct: f32, stage: &str| {
                if req_tracker_progress.load(Ordering::SeqCst) != req_id {
                    return;
                }
                // Smart threshold: only show UI card if >= 100ms elapsed (or already shown)
                if progress_shown_clone.load(Ordering::SeqCst)
                    || start_time.elapsed() >= Duration::from_millis(100)
                {
                    progress_shown_clone.store(true, Ordering::SeqCst);
                    let ui_weak = ui_weak_progress.clone();
                    let title = fname.clone();
                    let stg = stage.to_string();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.set_is_loading(true);
                            ui.set_loading_title(title.into());
                            ui.set_loading_progress(pct);
                            ui.set_loading_stage(stg.into());
                        }
                    });
                }
            };

            let decode_res = decode_photo_with_progress(&path, progress_cb);

            if req_tracker.load(Ordering::SeqCst) != req_id {
                return;
            }

            match decode_res {
                Ok(photo) => {
                    let (rgba_buf, w, h) = make_rgba_pixel_buffer(&photo.image);
                    let thumb_buf = make_thumbnail_buffer(&photo.image);
                    let cached = CachedPhoto {
                        photo: Arc::new(photo),
                        rgba_buf,
                        w,
                        h,
                        thumb_buf,
                    };

                    let ui_weak = ui_weak.clone();
                    let state = state_clone.clone();
                    let path_clone = path.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if req_tracker.load(Ordering::SeqCst) != req_id {
                            return;
                        }
                        if let Some(ui) = ui_weak.upgrade() {
                            let mut s = state.lock().unwrap();
                            s.image_cache.insert(path_clone.clone(), cached.clone());
                            s.apply_cached_photo(
                                &ui,
                                index,
                                &path_clone,
                                cached,
                            );
                            ui.set_is_loading(false);
                            is_loading_main.store(false, Ordering::SeqCst);
                        }
                        Self::preload_neighbors(&state, index);
                    });
                }
                Err(err) => {
                    eprintln!("Error decoding image {:?}: {:?}", path, err);
                    let ui_weak = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if req_tracker.load(Ordering::SeqCst) != req_id {
                            return;
                        }
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.set_is_loading(false);
                            ui.set_current_filename(format!("Error loading: {:?}", err).into());
                            is_loading_main.store(false, Ordering::SeqCst);
                        }
                    });
                }
            }
        });
    }

    /// Applies a cached photo to the UI and internal viewport state, triggering a crossfade transition if enabled.
    pub fn apply_cached_photo(
        &mut self,
        ui: &AppWindow,
        index: usize,
        path: &Path,
        cached: CachedPhoto,
    ) {
        let slint_img = slint::Image::from_rgba8(cached.rgba_buf);
        let slint_thumb = slint::Image::from_rgba8(cached.thumb_buf);
        let (w, h) = (cached.w, cached.h);
        let photo = cached.photo;

        let old_has_image = ui.get_has_image();
        let transition_mode = ui.get_transition_mode().to_string();

        let is_transitioning = old_has_image
            && (transition_mode == "crossfade" || transition_mode == "slide")
            && self.displayed_index != Some(index);

        let slide_direction = if let Some(prev_idx) = self.displayed_index {
            if index >= prev_idx {
                1.0f32
            } else {
                -1.0f32
            }
        } else {
            1.0f32
        };

        if is_transitioning {
            // Save currently displayed photo as previous outgoing photo
            ui.set_prev_image(ui.get_current_image());
            ui.set_prev_image_offset_x(ui.get_image_offset_x());
            ui.set_prev_image_offset_y(ui.get_image_offset_y());
            ui.set_prev_image_display_width(ui.get_image_display_width());
            ui.set_prev_image_display_height(ui.get_image_display_height());
            ui.set_has_prev_image(true);
            ui.set_slide_direction(slide_direction);
            ui.set_crossfade_progress(0.0);
        } else {
            ui.set_has_prev_image(false);
            ui.set_slide_direction(slide_direction);
            ui.set_crossfade_progress(1.0);
        }

        // Update UI state with new photo
        ui.set_has_image(true);
        ui.set_current_image(slint_img);
        ui.invoke_set_thumbnail(index as i32, slint_thumb);
        ui.set_current_filename(
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .into(),
        );
        ui.set_current_dimensions(format!("{} × {} px", w, h).into());
        ui.set_current_format(photo.format_name.clone().into());
        ui.set_current_file_size(format_file_size(photo.file_size_bytes).into());
        ui.set_current_index_text(
            format!("{} / {}", index + 1, self.session_files.len()).into(),
        );

        // Populate grouped EXIF & RAW metadata entries
        let mut exif_entries = Vec::new();

        let mut add_section = |title: &str, items: Vec<(&str, Option<String>)>| {
            let valid_items: Vec<(&str, String)> = items
                .into_iter()
                .filter_map(|(label, val)| val.map(|v| (label, v)))
                .collect();
            if !valid_items.is_empty() {
                exif_entries.push(ExifEntry {
                    label: title.into(),
                    value: "".into(),
                    is_header: true,
                });
                for (label, val) in valid_items {
                    exif_entries.push(ExifEntry {
                        label: label.into(),
                        value: val.into(),
                        is_header: false,
                    });
                }
            }
        };

        // 1. Camera & Actuations
        add_section(
            "CAMERA & ACTUATIONS",
            vec![
                ("Make", photo.exif.camera_make.clone()),
                ("Camera", photo.exif.camera_model.clone()),
                ("Shutter Count", photo.exif.shutter_count.clone()),
                ("Image / Shot #", photo.exif.image_number.clone()),
                ("Drive Mode", photo.exif.drive_mode.clone()),
                ("Focus Mode", photo.exif.focus_mode.clone()),
                ("AF System", photo.exif.af_system.clone()),
                ("Body Serial", photo.exif.body_serial.clone()),
                ("Internal Serial", photo.exif.internal_serial.clone()),
                ("Firmware", photo.exif.firmware.clone()),
                ("Camera Temp", photo.exif.camera_temp.clone()),
            ],
        );

        // 2. Exposure & Metering
        add_section(
            "EXPOSURE & METERING",
            vec![
                ("Shutter", photo.exif.shutter_speed.clone()),
                ("Aperture", photo.exif.aperture.clone()),
                ("ISO", photo.exif.iso.clone()),
                ("Program", photo.exif.exposure_program.clone()),
                ("Exp. Mode", photo.exif.exposure_mode.clone()),
                ("Exp. Bias", photo.exif.exposure_bias.clone()),
                ("Metering", photo.exif.metering_mode.clone()),
                ("White Balance", photo.exif.white_balance.clone()),
                ("Light Source", photo.exif.light_source.clone()),
                ("Color Temp", photo.exif.color_temp.clone()),
                ("Picture Style", photo.exif.picture_style.clone()),
                ("Flash", photo.exif.flash.clone()),
            ],
        );

        // 3. Optics & Lens
        add_section(
            "OPTICS & LENS",
            vec![
                ("Lens Make", photo.exif.lens_make.clone()),
                ("Lens", photo.exif.lens_model.clone()),
                ("Lens Spec", photo.exif.lens_spec.clone()),
                ("Focal Length", photo.exif.focal_length.clone()),
                ("35mm Equiv.", photo.exif.focal_length_35mm.clone()),
                ("Max Aperture", photo.exif.max_aperture.clone()),
                ("Subject Dist.", photo.exif.subject_distance.clone()),
                ("Lens Serial", photo.exif.lens_serial.clone()),
            ],
        );

        // 4. RAW Sensor Pipeline (only present for Camera RAW files)
        if let Some(ref raw) = photo.exif.raw_sensor {
            add_section(
                "RAW SENSOR PIPELINE",
                vec![
                    (
                        "Sensor Model",
                        if !raw.clean_model.is_empty() {
                            Some(format!("{} {}", raw.clean_make, raw.clean_model))
                        } else {
                            None
                        },
                    ),
                    ("Raw Sensor", Some(format!("{} × {} px", raw.sensor_width, raw.sensor_height))),
                    ("Active Area", Some(format!("{} × {} px", raw.active_width, raw.active_height))),
                    ("CFA Pattern", Some(format!("{} (Bayer)", raw.cfa_pattern))),
                    ("ADC Depth", Some(raw.bit_depth_est.clone())),
                    ("Black Level", Some(raw.black_level.clone())),
                    ("White Level", Some(raw.white_level.clone())),
                    ("As-Shot WB", Some(raw.wb_multipliers.clone())),
                ],
            );
        }

        // 5. Author, GPS & File
        add_section(
            "FILE, AUTHOR & GPS",
            vec![
                ("Date Taken", photo.exif.date_taken.clone()),
                ("Digitized", photo.exif.date_digitized.clone()),
                ("Artist / Owner", photo.exif.artist.clone()),
                ("Copyright", photo.exif.copyright.clone()),
                ("Color Space", photo.exif.color_space.clone()),
                ("GPS Coords", photo.exif.gps_coords.clone()),
                ("GPS Altitude", photo.exif.gps_altitude.clone()),
                ("Dimensions", Some(format!("{} × {} px", w, h))),
                (
                    "Megapixels",
                    Some(format!("{:.1} MP", (w as f32 * h as f32) / 1_000_000.0)),
                ),
                ("Format", Some(photo.format_name.clone())),
                ("File Size", Some(format_file_size(photo.file_size_bytes))),
                ("Full Path", Some(path.to_string_lossy().to_string())),
            ],
        );

        let exif_model = Rc::new(VecModel::from(exif_entries));
        ui.set_exif_entries(ModelRc::from(exif_model));

        // Sync live canvas dimensions before fitting
        let live_w = ui.get_live_canvas_width();
        let live_h = ui.get_live_canvas_height();
        if live_w > 10.0 && live_h > 10.0 {
            self.viewport.vp_w = live_w;
            self.viewport.vp_h = live_h;
        }

        // Configure viewport and fit image
        self.viewport.set_image_dimensions(w as f32, h as f32);
        self.sync_viewport_to_ui(ui);

        // Trigger smooth 60 FPS transition (Slide or Crossfade) if enabled
        if is_transitioning {
            let start = Instant::now();
            let trans_id = self.transition_id.fetch_add(1, Ordering::SeqCst) + 1;
            let trans_id_tracker = self.transition_id.clone();
            let ui_weak = ui.as_weak();
            let timer = Rc::new(RefCell::new(slint::Timer::default()));
            let timer_clone = timer.clone();
            let is_slide = transition_mode == "slide";
            let duration_secs = if is_slide { 0.350f32 } else { 0.500f32 };

            timer.borrow_mut().start(
                slint::TimerMode::Repeated,
                Duration::from_millis(16),
                move || {
                    if trans_id_tracker.load(Ordering::SeqCst) != trans_id {
                        timer_clone.borrow().stop();
                        return;
                    }
                    let elapsed = start.elapsed().as_secs_f32();
                    let t = (elapsed / duration_secs).min(1.0);

                    // Smooth cubic ease-in-out curve for position
                    let progress = if t < 0.5 {
                        4.0 * t * t * t
                    } else {
                        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                    };

                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_crossfade_progress(progress);
                        if t >= 1.0 {
                            ui.set_has_prev_image(false);
                            ui.set_crossfade_progress(1.0);
                            timer_clone.borrow().stop();
                        }
                    } else {
                        timer_clone.borrow().stop();
                    }
                },
            );
        }

        self.displayed_index = Some(index);
        self.current_photo = Some((*photo).clone());
    }

    /// Applies the newly decoded photo to the UI and internal viewport state.
    #[allow(dead_code)]
    pub fn apply_loaded_photo(
        &mut self,
        ui: &AppWindow,
        index: usize,
        path: &Path,
        photo: LoadedPhoto,
        rgba_buf: slint::SharedPixelBuffer<slint::Rgba8Pixel>,
        w: u32,
        h: u32,
        thumb_buf: slint::SharedPixelBuffer<slint::Rgba8Pixel>,
    ) {
        let cached = CachedPhoto {
            photo: Arc::new(photo),
            rgba_buf,
            w,
            h,
            thumb_buf,
        };
        self.apply_cached_photo(ui, index, path, cached);
    }

    pub fn navigate_prev(state: &Arc<Mutex<AppState>>, ui: &AppWindow) {
        let idx_opt = state.lock().unwrap().active_index;
        if let Some(idx) = idx_opt {
            if idx > 0 {
                Self::select_index(state, ui, idx - 1);
            }
        }
    }

    pub fn navigate_next(state: &Arc<Mutex<AppState>>, ui: &AppWindow) {
        let (idx_opt, total) = {
            let s = state.lock().unwrap();
            (s.active_index, s.session_files.len())
        };
        if let Some(idx) = idx_opt {
            if idx + 1 < total {
                Self::select_index(state, ui, idx + 1);
            }
        }
    }

    pub fn clear_session(&mut self, ui: &AppWindow) {
        self.session_files.clear();
        self.active_index = None;
        self.displayed_index = None;
        self.current_photo = None;
        self.image_cache.clear();
        self.preload_request_id.fetch_add(1, Ordering::SeqCst);
        self.transition_id.fetch_add(1, Ordering::SeqCst);
        self.viewport = Viewport::new();
        self.is_loading_main.store(false, Ordering::SeqCst);
        ui.set_is_loading(false);
        ui.set_has_image(false);
        ui.set_has_prev_image(false);
        ui.set_crossfade_progress(1.0);
        ui.set_current_image(slint::Image::default());
        ui.set_prev_image(slint::Image::default());
        ui.set_current_filename("No photo open".into());
        ui.set_current_dimensions("".into());
        ui.set_current_format("".into());
        ui.set_current_file_size("".into());
        ui.set_current_index_text("".into());
        ui.set_show_filmstrip(false);
        ui.set_filmstrip_items(ModelRc::from(Rc::new(VecModel::default())));
        ui.set_exif_entries(ModelRc::from(Rc::new(VecModel::default())));
        self.sync_viewport_to_ui(ui);
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
    pub fn spawn_thumbnail_generation(
        &self,
        ui_weak: slint::Weak<AppWindow>,
        paths: Vec<PathBuf>,
    ) {
        let current_gen = self.generation_id.fetch_add(1, Ordering::SeqCst) + 1;
        let gen_tracker = self.generation_id.clone();
        let is_loading_main = self.is_loading_main.clone();

        std::thread::spawn(move || {
            for (idx, path) in paths.into_iter().enumerate() {
                // If a newer session has started, abort this batch
                if gen_tracker.load(Ordering::SeqCst) != current_gen {
                    return;
                }

                // If the main photo is currently decoding, wait to give full CPU to the active photo
                while is_loading_main.load(Ordering::SeqCst) {
                    if gen_tracker.load(Ordering::SeqCst) != current_gen {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_neighbor_candidates_window() {
        let total = 10;
        let get_candidates = |idx: usize| -> Vec<usize> {
            let mut candidates = Vec::new();
            if idx + 1 < total {
                candidates.push(idx + 1);
            }
            if idx > 0 {
                candidates.push(idx - 1);
            }
            if idx + 2 < total {
                candidates.push(idx + 2);
            }
            if idx >= 2 {
                candidates.push(idx - 2);
            }
            candidates
        };

        // First image in sequence: only forward neighbors available
        assert_eq!(get_candidates(0), vec![1, 2]);

        // Middle image: balanced +1, -1, +2, -2
        assert_eq!(get_candidates(4), vec![5, 3, 6, 2]);

        // Last image: only backward neighbors available
        assert_eq!(get_candidates(9), vec![8, 7]);
    }

    #[test]
    fn test_cache_pruning_bounds() {
        let mut state = AppState::new();
        state.session_files = (0..10).map(|i| PathBuf::from(format!("img_{}.jpg", i))).collect();

        // Simulate population of cache with artificial entries
        for path in &state.session_files {
            state.image_cache.insert(
                path.clone(),
                CachedPhoto {
                    photo: Arc::new(LoadedPhoto {
                        image: image::DynamicImage::new_rgb8(1, 1),
                        format_name: "JPEG".into(),
                        exif: Default::default(),
                        file_size_bytes: 100,
                    }),
                    rgba_buf: slint::SharedPixelBuffer::new(1, 1),
                    w: 1,
                    h: 1,
                    thumb_buf: slint::SharedPixelBuffer::new(1, 1),
                },
            );
        }
        assert_eq!(state.image_cache.len(), 10);

        // When navigating to index 4, keep_paths = {4, 5, 3, 6, 2}
        let current_idx = 4;
        let total = state.session_files.len();
        let mut candidates = Vec::new();
        if current_idx + 1 < total { candidates.push(current_idx + 1); }
        if current_idx > 0 { candidates.push(current_idx - 1); }
        if current_idx + 2 < total { candidates.push(current_idx + 2); }
        if current_idx >= 2 { candidates.push(current_idx - 2); }

        let keep_paths: HashSet<PathBuf> = candidates
            .iter()
            .chain(std::iter::once(&current_idx))
            .filter_map(|&idx| state.session_files.get(idx).cloned())
            .collect();

        state.image_cache.retain(|path, _| keep_paths.contains(path));

        // Bounded to 5: active photo + up to 4 neighbors
        assert_eq!(state.image_cache.len(), 5);
        assert!(state.image_cache.contains_key(&PathBuf::from("img_4.jpg")));
        assert!(state.image_cache.contains_key(&PathBuf::from("img_5.jpg")));
        assert!(state.image_cache.contains_key(&PathBuf::from("img_3.jpg")));
        assert!(state.image_cache.contains_key(&PathBuf::from("img_6.jpg")));
        assert!(state.image_cache.contains_key(&PathBuf::from("img_2.jpg")));
        assert!(!state.image_cache.contains_key(&PathBuf::from("img_0.jpg")));
        assert!(!state.image_cache.contains_key(&PathBuf::from("img_9.jpg")));
    }

    #[test]
    fn test_slide_direction() {
        let calc_direction = |prev_idx: Option<usize>, new_idx: usize| -> f32 {
            if let Some(prev) = prev_idx {
                if new_idx >= prev {
                    1.0f32
                } else {
                    -1.0f32
                }
            } else {
                1.0f32
            }
        };

        // Forward navigation (Next / jump forward) -> 1.0 (slides out to left, in from right)
        assert_eq!(calc_direction(Some(0), 1), 1.0);
        assert_eq!(calc_direction(Some(2), 5), 1.0);

        // Backward navigation (Prev / jump backward) -> -1.0 (slides out to right, in from left)
        assert_eq!(calc_direction(Some(5), 4), -1.0);
        assert_eq!(calc_direction(Some(3), 0), -1.0);

        // First image opened (no previous index) -> defaults to 1.0
        assert_eq!(calc_direction(None, 0), 1.0);
    }
}
