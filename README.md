# Photodile

<p align="center">
  <strong>A high-performance, cross-platform photography visualizer built in 100% pure Rust and Slint.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-orange?style=flat-square&logo=rust" alt="Rust" />
  <img src="https://img.shields.io/badge/GUI-Slint-blue?style=flat-square" alt="Slint" />
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20Linux-lightgrey?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/badge/codecs-100%25%20Pure%20Rust-green?style=flat-square" alt="Pure Rust Codecs" />
  <img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="License: MIT" />
</p>

---

## Highlights

- **Photoshop / Lightroom-Style Scrubby Zoom**: Hold <kbd>Space</kbd> + <kbd>Ctrl</kbd> and drag horizontally to zoom in and out smoothly around your initial click point.
- **Fluid 1:1 Canvas Panning**: Hold <kbd>Space</kbd> and drag with left click to glide across high-resolution imagery.
- **Cursor-Anchored Wheel Zoom**: Scroll the mouse wheel to zoom centered directly under your cursor.
- **Smart Filmstrip**: Automatically reveals when 2+ photos are opened, and hides for single photos to maximize viewport space.
- **Asynchronous Thumbnail Pipeline**: Generates filmstrip previews in the background so the UI stays buttery smooth even with hundreds of high-megapixel photos.
- **"Convert to..." Modal & Quick Save**: Convert photos between formats with an interactive in-app dialog featuring a **quality slider** for JPEG & AVIF, or use standard **Save As...**.
- **Broad Format Compatibility**: Pure-Rust decoding for standard raster, vector SVG, Photoshop PSD, JPEG XL, and Camera RAW files.
- **Camera EXIF Inspector & Auto-Orientation**: Automatically corrects portrait rotation tags (1–8) and displays full camera, lens, and exposure metadata.

---

## Features

### 🔍 Canvas Navigation & View Controls

| Shortcut / Gesture | Action |
| --- | --- |
| <kbd>Space</kbd> + **Left Click Drag** | **Pan Tool**: Smoothly drag and pan the canvas |
| <kbd>Space</kbd> + <kbd>Ctrl</kbd> + **Drag Right** | **Scrubby Zoom In**: Zooms in anchored to the initial click point |
| <kbd>Space</kbd> + <kbd>Ctrl</kbd> + **Drag Left** | **Scrubby Zoom Out**: Zooms out anchored to the initial click point |
| **Mouse Wheel Scroll** | **Live Zoom**: Smooth zoom centered at current mouse cursor position |
| <kbd>0</kbd> or <kbd>F</kbd> | **Fit to View**: Resets image to fit viewport bounds |
| <kbd>1</kbd> | **100% View**: 1:1 pixel scale centered on viewport |
| <kbd>T</kbd> | Toggle bottom **Filmstrip** |
| <kbd>I</kbd> | Toggle **EXIF & Metadata Sidebar** |
| <kbd>←</kbd> / <kbd>→</kbd> | Navigate to **Previous / Next** image in the filmstrip |

---

### 📂 File & Folder Loading

- **Single Photo**: Open an image via file picker, CLI argument, or drag-and-drop. The filmstrip auto-hides so you get 100% canvas real estate.
- **Multi-File Selection**: Select multiple photos at once or use **Add Images...** to append to your current session.
- **Folder Scanning**: Open a directory or drag a folder directly onto the window. Automatically loads all supported top-level photos with natural filename sorting (`img1`, `img2`, `img10`).
- **OS Drag & Drop**: Drag images or directories directly from Windows Explorer or Linux file managers with visual drop-zone feedback.

---

### 🎞️ Bottom Filmstrip

- Displays thumbnail cards with image preview and truncated filename.
- Highlights active photo with blue accent outline.
- Instant navigation using arrow keys (<kbd>←</kbd> / <kbd>→</kbd>) or clicking thumbnails directly.
- Non-blocking background worker pool renders thumbnails asynchronously.

---

### 💾 Export & Format Conversion

- **Save As...**: Native file picker with automatic format detection from the chosen file extension.
- **Convert to...**: In-app modal dialog:
  - Choose target format: **PNG, JPEG, WebP, AVIF, TIFF, QOI, BMP, OpenEXR, TGA, Farbfeld, PNM, ICO**.
  - Adjust **Quality slider (1–100%)** for lossy formats (**JPEG**, **AVIF**).
  - Preview source details before confirming export destination.

---

### 📷 Comprehensive Format Support (100% Pure Rust)

Zero C/C++ runtime or build dependencies required.

- **Standard & Pro Raster**: JPEG, PNG, WebP, TIFF, OpenEXR (`.exr`), Radiance HDR (`.hdr`), BMP, GIF, ICO, TGA, QOI, PNM (`.pbm`, `.pgm`, `.ppm`, `.pam`), DDS, Farbfeld (`.ff`)
- **Vector Graphics**: SVG / SVGZ (rasterized via `resvg` & `tiny-skia`)
- **Adobe Photoshop**: PSD (`.psd`) composite layer rendering via `psd`
- **Next-Gen Web**: JPEG XL (`.jxl` via `jxl-oxide`)
- **Camera RAW**: Sensor decoding via `rawloader` and sRGB demosaicing via `imagepipe`
  - `.dng`, `.cr2`, `.cr3` (Canon)
  - `.nef`, `.nrw` (Nikon)
  - `.arw`, `.srf`, `.sr2` (Sony)
  - `.orf` (Olympus)
  - `.rw2` (Panasonic)
  - `.pef` (Pentax)
  - `.raf` (Fujifilm)
  - `.srw`, `.erf`, `.kdc`, `.dcr`, `.mos`, `.mef`, `.mrw`

---

### 📊 EXIF & Photography Metadata

- Automatic EXIF orientation tag handling (`1..=8`) so portrait photos always open upright.
- Collapsible sidebar inspector displaying:
  - **Camera Make & Model**
  - **Lens Model**
  - **Focal Length**
  - **Aperture ($f$-number)**
  - **Shutter Speed**
  - **ISO Sensitivity**
  - **Date Taken**
  - **Resolution & Megapixels**
  - **File Size & Format**

---

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (version 1.80+ or latest stable)

### Build & Run

#### On Windows:
```powershell
# Run directly with Cargo
cargo run --release

# Or build the standalone release binary
cargo build --release
.\target\release\photodile.exe
```

#### On Linux:
```bash
# Run directly with Cargo
cargo run --release

# Or build the standalone release binary
cargo build --release
./target/release/photodile
```

### Command-Line Usage

You can open images or folders directly from your terminal:

```bash
# Open a single photo
photodile photo.jpg

# Open multiple photos
photodile image1.png image2.webp image3.arw

# Open an entire folder
photodile "C:\Users\Username\Pictures\Trip"
```

---

## Running Tests

Run the test suite covering viewport geometry math, gestures, format detection, and roundtrip conversion:

```bash
cargo test
```

---

## Tech Stack

- **GUI Framework**: [Slint](https://slint.dev/)
- **Window Backend**: [Winit](https://github.com/rust-windowing/winit)
- **Image Processing**: [image](https://crates.io/crates/image), [resvg](https://crates.io/crates/resvg), [psd](https://crates.io/crates/psd), [jxl-oxide](https://crates.io/crates/jxl-oxide), [rawloader](https://crates.io/crates/rawloader), [imagepipe](https://crates.io/crates/imagepipe)
- **EXIF Parser**: [kamadak-exif](https://crates.io/crates/kamadak-exif)
- **File Dialogs**: [rfd](https://crates.io/crates/rfd)

---

## License

This project is open source and available under the [MIT License](LICENSE).

