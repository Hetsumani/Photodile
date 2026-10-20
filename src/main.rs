#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Photodile - Pure-Rust Photography Visualizer
//!
//! Main application entry point integrating Slint UI, multi-format image engine,
//! interactive viewport navigation (Pan, Scrubby Zoom, Wheel Zoom), OS drag-and-drop,
//! and filmstrip navigation.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use i_slint_backend_winit::WinitWindowAccessor;
use slint::ComponentHandle;

mod exif_meta;
mod formats;
mod state;
mod viewport;

slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = AppWindow::new()?;
    let state = Rc::new(RefCell::new(state::AppState::new()));

    // 1. File Opening Callbacks
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_open_single_file(move || {
            let file = rfd::FileDialog::new()
                .set_title("Open Photo")
                .add_filter("Images", formats::SUPPORTED_EXTENSIONS)
                .pick_file();

            if let (Some(path), Some(ui)) = (file, ui_weak.upgrade()) {
                state.borrow_mut().load_paths(&ui, vec![path], false);
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_open_multi_files(move || {
            let files = rfd::FileDialog::new()
                .set_title("Open Multiple Photos")
                .add_filter("Images", formats::SUPPORTED_EXTENSIONS)
                .pick_files();

            if let (Some(paths), Some(ui)) = (files, ui_weak.upgrade()) {
                state.borrow_mut().load_paths(&ui, paths, false);
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_open_folder(move || {
            let folder = rfd::FileDialog::new()
                .set_title("Open Photo Folder")
                .pick_folder();

            if let (Some(path), Some(ui)) = (folder, ui_weak.upgrade()) {
                state.borrow_mut().load_paths(&ui, vec![path], false);
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_add_images(move || {
            let files = rfd::FileDialog::new()
                .set_title("Add Photos to Session")
                .add_filter("Images", formats::SUPPORTED_EXTENSIONS)
                .pick_files();

            if let (Some(paths), Some(ui)) = (files, ui_weak.upgrade()) {
                state.borrow_mut().load_paths(&ui, paths, true);
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_clear_session(move || {
            if let Some(ui) = ui_weak.upgrade() {
                state.borrow_mut().clear_session(&ui);
            }
        });
    }

    // 2. Export / Save As Callbacks
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_save_as(move || {
            let s = state.borrow();
            let photo = match s.current_photo.as_ref() {
                Some(p) => p,
                None => return,
            };

            let default_stem = s
                .active_index
                .and_then(|idx| s.session_files.get(idx))
                .and_then(|p| p.file_stem())
                .and_then(|s| s.to_str())
                .unwrap_or("export");

            let dialog = rfd::FileDialog::new()
                .set_title("Save Photo As")
                .set_file_name(format!("{}.png", default_stem))
                .add_filter("PNG Image (*.png)", &["png"])
                .add_filter("JPEG Image (*.jpg, *.jpeg)", &["jpg", "jpeg"])
                .add_filter("WebP Image (*.webp)", &["webp"])
                .add_filter("AVIF Image (*.avif)", &["avif"])
                .add_filter("TIFF Image (*.tif, *.tiff)", &["tif", "tiff"])
                .add_filter("QOI Image (*.qoi)", &["qoi"])
                .add_filter("BMP Image (*.bmp)", &["bmp"])
                .add_filter("OpenEXR HDR (*.exr)", &["exr"])
                .add_filter("TGA Image (*.tga)", &["tga"]);

            if let Some(dest) = dialog.save_file() {
                let ext = dest
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("png");
                let fmt = formats::ExportFormat::from_extension(ext);
                if let Err(e) = formats::save_or_convert_image(&photo.image, &dest, fmt, 92) {
                    eprintln!("Failed to save image to {:?}: {:?}", dest, e);
                } else if let Some(ui) = ui_weak.upgrade() {
                    ui.set_current_filename(
                        format!(
                            "Saved: {}",
                            dest.file_name().and_then(|s| s.to_str()).unwrap_or("")
                        )
                        .into(),
                    );
                }
            }
        });
    }

    // "Convert to..." Dialog Callbacks
    {
        let ui_weak = ui.as_weak();
        ui.on_open_convert_modal(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_show_convert_modal(true);
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        ui.on_close_convert_modal(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_show_convert_modal(false);
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_confirm_convert(move |fmt_index, quality| {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_show_convert_modal(false);
            }

            let s = state.borrow();
            let photo = match s.current_photo.as_ref() {
                Some(p) => p,
                None => return,
            };

            let format = formats::ExportFormat::from_index(fmt_index);
            let default_stem = s
                .active_index
                .and_then(|idx| s.session_files.get(idx))
                .and_then(|p| p.file_stem())
                .and_then(|s| s.to_str())
                .unwrap_or("converted");

            let dialog = rfd::FileDialog::new()
                .set_title("Convert & Save As")
                .set_file_name(format!("{}.{}", default_stem, format.extension()))
                .add_filter(
                    &format!("{:?} Image", format),
                    &[format.extension()],
                );

            if let Some(dest) = dialog.save_file() {
                if let Err(e) =
                    formats::save_or_convert_image(&photo.image, &dest, format, quality as u8)
                {
                    eprintln!("Failed to convert image to {:?}: {:?}", dest, e);
                } else if let Some(ui) = ui_weak.upgrade() {
                    ui.set_current_filename(
                        format!(
                            "Converted: {}",
                            dest.file_name().and_then(|s| s.to_str()).unwrap_or("")
                        )
                        .into(),
                    );
                }
            }
        });
    }

    // 3. Viewport Zoom & Pan Callbacks
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_zoom_in(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let mut s = state.borrow_mut();
                s.viewport.step_zoom(true);
                s.sync_viewport_to_ui(&ui);
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_zoom_out(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let mut s = state.borrow_mut();
                s.viewport.step_zoom(false);
                s.sync_viewport_to_ui(&ui);
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_zoom_fit(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let mut s = state.borrow_mut();
                let live_w = ui.get_live_canvas_width();
                let live_h = ui.get_live_canvas_height();
                if live_w > 10.0 && live_h > 10.0 {
                    s.viewport.vp_w = live_w;
                    s.viewport.vp_h = live_h;
                }
                s.viewport.fit_to_viewport();
                s.sync_viewport_to_ui(&ui);
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_zoom_100(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let mut s = state.borrow_mut();
                s.viewport.set_zoom_100();
                s.sync_viewport_to_ui(&ui);
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_canvas_resized(move |w, h| {
            if let Some(ui) = ui_weak.upgrade() {
                let mut s = state.borrow_mut();
                s.viewport.set_viewport_dimensions(w, h);
                s.sync_viewport_to_ui(&ui);
            }
        });
    }

    // Interactive pointer gestures (Pan: Space+Drag, Scrubby Zoom: Space+Ctrl+Drag)
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_canvas_pointer_down(move |x, y, _btn| {
            if let Some(ui) = ui_weak.upgrade() {
                let space = ui.get_space_held();
                let ctrl = ui.get_ctrl_held();
                let started = state.borrow_mut().viewport.begin_pointer_drag(x, y, space, ctrl);
                if started {
                    ui.set_is_dragging_canvas(true);
                }
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_canvas_pointer_move(move |x, y| {
            let mut s = state.borrow_mut();
            if s.viewport.update_pointer_drag(x, y) {
                if let Some(ui) = ui_weak.upgrade() {
                    s.sync_viewport_to_ui(&ui);
                }
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_canvas_pointer_up(move |_x, _y| {
            state.borrow_mut().viewport.end_pointer_drag();
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_is_dragging_canvas(false);
            }
        });
    }

    // Mouse wheel cursor-anchored zoom
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_canvas_scroll(move |delta_y, cursor_x, cursor_y| {
            if let Some(ui) = ui_weak.upgrade() {
                let mut s = state.borrow_mut();
                s.viewport.wheel_zoom(delta_y, cursor_x, cursor_y);
                s.sync_viewport_to_ui(&ui);
            }
        });
    }

    // 4. Filmstrip Navigation & Toggles
    {
        let ui_weak = ui.as_weak();
        ui.on_toggle_filmstrip(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_show_filmstrip(!ui.get_show_filmstrip());
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        ui.on_toggle_info(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_show_info(!ui.get_show_info());
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_select_index(move |idx| {
            if let Some(ui) = ui_weak.upgrade() {
                state.borrow_mut().select_index(&ui, idx as usize);
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_navigate_prev(move || {
            if let Some(ui) = ui_weak.upgrade() {
                state.borrow_mut().navigate_prev(&ui);
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_navigate_next(move || {
            if let Some(ui) = ui_weak.upgrade() {
                state.borrow_mut().navigate_next(&ui);
            }
        });
    }

    // 5. Frameless Window Controls & Resize
    {
        let ui_weak = ui.as_weak();
        ui.on_window_drag(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.window().with_winit_window(|winit_win| {
                    let _ = winit_win.drag_window();
                });
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        ui.on_window_minimize(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.window().with_winit_window(|winit_win| {
                    winit_win.set_minimized(true);
                });
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        ui.on_window_toggle_maximize(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.window().with_winit_window(|winit_win| {
                    let is_max = !winit_win.is_maximized();
                    winit_win.set_maximized(is_max);
                    ui.set_is_window_maximized(is_max);
                });
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        ui.on_window_close(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let _ = ui.hide();
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        ui.on_window_resize(move |dir_idx| {
            if let Some(ui) = ui_weak.upgrade() {
                let direction = match dir_idx {
                    1 => winit::window::ResizeDirection::North,
                    2 => winit::window::ResizeDirection::South,
                    3 => winit::window::ResizeDirection::East,
                    4 => winit::window::ResizeDirection::West,
                    5 => winit::window::ResizeDirection::NorthEast,
                    6 => winit::window::ResizeDirection::NorthWest,
                    7 => winit::window::ResizeDirection::SouthEast,
                    8 => winit::window::ResizeDirection::SouthWest,
                    _ => return,
                };
                ui.window().with_winit_window(|winit_win| {
                    let _ = winit_win.drag_resize_window(direction);
                });
            }
        });
    }

    // 6. OS File Drag & Drop Interception and Window Icon via Winit Backend Hook
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        let dropped_batch = Rc::new(RefCell::new(Vec::<PathBuf>::new()));
        let timer = Rc::new(RefCell::new(slint::Timer::default()));
        let icon_set = Rc::new(RefCell::new(false));

        ui.window().on_winit_window_event(move |window, event| {
            if !*icon_set.borrow() {
                *icon_set.borrow_mut() = true;
                window.with_winit_window(|winit_win| {
                    if let Ok(icon_bytes) = std::fs::read("images/iconV04.ico") {
                        if let Ok(img) = image::load_from_memory_with_format(&icon_bytes, image::ImageFormat::Ico) {
                            let rgba = img.into_rgba8();
                            let (w, h) = (rgba.width(), rgba.height());
                            if let Ok(icon) = winit::window::Icon::from_rgba(rgba.into_raw(), w, h) {
                                winit_win.set_window_icon(Some(icon));
                            }
                        }
                    }
                });
            }

            match event {
                winit::event::WindowEvent::Resized(_) => {
                    if let Some(ui) = ui_weak.upgrade() {
                        window.with_winit_window(|winit_win| {
                            ui.set_is_window_maximized(winit_win.is_maximized());
                        });
                    }
                }
                winit::event::WindowEvent::HoveredFile(_) => {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_is_drag_hovered(true);
                    }
                }
                winit::event::WindowEvent::HoveredFileCancelled => {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_is_drag_hovered(false);
                    }
                }
                winit::event::WindowEvent::DroppedFile(path) => {
                    dropped_batch.borrow_mut().push(path.clone());
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_is_drag_hovered(false);
                    }

                    // Debounce/batch multiple simultaneously dropped files and append to session
                    let ui_weak = ui_weak.clone();
                    let state = state.clone();
                    let batch = dropped_batch.clone();
                    timer.borrow_mut().start(
                        slint::TimerMode::SingleShot,
                        std::time::Duration::from_millis(30),
                        move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let paths: Vec<PathBuf> = batch.borrow_mut().drain(..).collect();
                                if !paths.is_empty() {
                                    state.borrow_mut().load_paths(&ui, paths, true);
                                }
                            }
                        },
                    );
                }
                _ => {}
            }
            i_slint_backend_winit::EventResult::Propagate
        });
    }

    // 6. Handle Command-Line Arguments on Startup
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        let initial_paths: Vec<PathBuf> = args.into_iter().map(PathBuf::from).collect();
        state.borrow_mut().load_paths(&ui, initial_paths, false);
    }

    ui.run()?;
    Ok(())
}

