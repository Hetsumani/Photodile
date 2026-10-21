#!/usr/bin/env bash
set -e

# Photodile - Linux User Installation Script (Nobara / Fedora / KDE Plasma 6 / GNOME)
# Installs binary, desktop entries, icons, Dolphin context menu, and MIME associations.

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"
ICON_BASE="${HOME}/.local/share/icons/hicolor"
PIXMAP_DIR="${HOME}/.local/share/pixmaps"
KIO_DIR="${HOME}/.local/share/kio/servicemenus"
MIMEAPPS="${HOME}/.config/mimeapps.list"

echo "==> 1. Compiling Photodile (Release Build)..."
cd "${REPO_DIR}"
cargo build --release

echo "==> 2. Installing binary to ${BIN_DIR}/photodile..."
mkdir -p "${BIN_DIR}"
cp -f "${REPO_DIR}/target/release/photodile" "${BIN_DIR}/photodile"
chmod 755 "${BIN_DIR}/photodile"

echo "==> 3. Generating and installing multi-resolution icons..."
mkdir -p "${PIXMAP_DIR}"
cp -f "${REPO_DIR}/images/iconV04.png" "${PIXMAP_DIR}/photodile.png"

for size in 16 32 48 64 128 256 512; do
    target_dir="${ICON_BASE}/${size}x${size}/apps"
    mkdir -p "${target_dir}"
    if command -v magick >/dev/null 2>&1; then
        magick "${REPO_DIR}/images/iconV04.png" -resize "${size}x${size}" "${target_dir}/photodile.png"
    elif command -v convert >/dev/null 2>&1; then
        convert "${REPO_DIR}/images/iconV04.png" -resize "${size}x${size}" "${target_dir}/photodile.png"
    else
        cp -f "${REPO_DIR}/images/iconV04.png" "${target_dir}/photodile.png"
    fi
done

echo "==> 4. Installing Desktop Application entry..."
mkdir -p "${APP_DIR}"

MIME_TYPES="image/jpeg;image/pjpeg;image/png;image/webp;image/tiff;image/avif;image/jxl;image/x-exr;image/vnd.radiance;image/bmp;image/x-ms-bmp;image/gif;image/x-icon;image/vnd.microsoft.icon;image/x-tga;image/qoi;image/x-portable-anymap;image/x-portable-bitmap;image/x-portable-graymap;image/x-portable-pixmap;image/x-dds;image/svg+xml;image/svg+xml-compressed;image/vnd.adobe.photoshop;image/x-psd;image/x-dcraw;image/x-adobe-dng;image/x-canon-cr2;image/x-canon-cr3;image/x-canon-crw;image/x-nikon-nef;image/x-nikon-nrw;image/x-sony-arw;image/x-sony-srf;image/x-sony-sr2;image/x-olympus-orf;image/x-panasonic-rw2;image/x-panasonic-raw;image/x-pentax-pef;image/x-fuji-raf;image/x-samsung-srw;image/x-epson-erf;image/x-kodak-kdc;image/x-kodak-dcr;image/x-minolta-mrw;inode/directory;"

cat <<EOF > "${APP_DIR}/photodile.desktop"
[Desktop Entry]
Type=Application
Name=Photodile
GenericName=Photo & RAW Viewer
Comment=Pure-Rust high-performance photo visualizer and Camera RAW developer
Exec=${BIN_DIR}/photodile %F
Icon=photodile
Terminal=false
StartupNotify=true
StartupWMClass=photodile
Categories=Graphics;2DGraphics;RasterGraphics;Photography;Viewer;
Keywords=photo;image;raw;cr2;nef;arw;dng;viewer;exif;
MimeType=${MIME_TYPES}
EOF
chmod 644 "${APP_DIR}/photodile.desktop"

echo "==> 5. Installing Dolphin / KDE ServiceMenu (Folder Context Menu)..."
mkdir -p "${KIO_DIR}"
cat <<EOF > "${KIO_DIR}/photodile-folder.desktop"
[Desktop Entry]
Actions=openInPhotodile;
Type=Service
MimeType=inode/directory;
X-KDE-Priority=TopLevel
X-KDE-StartupNotify=false

[Desktop Action openInPhotodile]
Name=Open Folder in Photodile
Name[es]=Abrir carpeta en Photodile
Icon=photodile
Exec=${BIN_DIR}/photodile %f
EOF
chmod 755 "${KIO_DIR}/photodile-folder.desktop"

echo "==> 6. Registering MIME Associations..."
python3 - <<'PYEOF'
import os
import configparser

mimeapps_path = os.path.expanduser("~/.config/mimeapps.list")
os.makedirs(os.path.dirname(mimeapps_path), exist_ok=True)

# Full list of types Photodile will be the default for (excluding PSD and inode/directory)
default_mimes = [
    "image/jpeg", "image/pjpeg", "image/png", "image/webp", "image/tiff",
    "image/avif", "image/jxl", "image/x-exr", "image/vnd.radiance",
    "image/bmp", "image/x-ms-bmp", "image/gif", "image/x-icon",
    "image/vnd.microsoft.icon", "image/x-tga", "image/qoi",
    "image/x-portable-anymap", "image/x-portable-bitmap", "image/x-portable-graymap",
    "image/x-portable-pixmap", "image/x-dds", "image/svg+xml", "image/svg+xml-compressed",
    "image/x-dcraw", "image/x-adobe-dng", "image/x-canon-cr2", "image/x-canon-cr3",
    "image/x-canon-crw", "image/x-nikon-nef", "image/x-nikon-nrw", "image/x-sony-arw",
    "image/x-sony-srf", "image/x-sony-sr2", "image/x-olympus-orf", "image/x-panasonic-rw2",
    "image/x-panasonic-raw", "image/x-pentax-pef", "image/x-fuji-raf", "image/x-samsung-srw",
    "image/x-epson-erf", "image/x-kodak-kdc", "image/x-kodak-dcr", "image/x-minolta-mrw"
]

# Secondary associations (Open With)
secondary_mimes = [
    "image/vnd.adobe.photoshop", "image/x-psd", "inode/directory"
]

config = configparser.ConfigParser(interpolation=None)
config.optionxform = str
if os.path.exists(mimeapps_path):
    config.read(mimeapps_path)

if not config.has_section("Default Applications"):
    config.add_section("Default Applications")
if not config.has_section("Added Associations"):
    config.add_section("Added Associations")

for m in default_mimes:
    config.set("Default Applications", m, "photodile.desktop")
    # Also ensure in Added Associations
    existing = config.get("Added Associations", m, fallback="")
    apps = [a for a in existing.split(";") if a.strip()]
    if "photodile.desktop" not in apps:
        apps.insert(0, "photodile.desktop")
    config.set("Added Associations", m, ";".join(apps) + ";")

for m in secondary_mimes:
    existing = config.get("Added Associations", m, fallback="")
    apps = [a for a in existing.split(";") if a.strip()]
    if "photodile.desktop" not in apps:
        apps.append("photodile.desktop")
    config.set("Added Associations", m, ";".join(apps) + ";")

with open(mimeapps_path, "w") as f:
    config.write(f, space_around_delimiters=False)

print("  MIME associations successfully updated in", mimeapps_path)
PYEOF

echo "==> 7. Updating desktop and icon caches..."
update-desktop-database "${APP_DIR}" 2>/dev/null || true
gtk-update-icon-cache -f -t "${ICON_BASE}" 2>/dev/null || true
kbuildsycoca6 2>/dev/null || true

echo ""
echo "=========================================================="
echo " Photodile release build installed successfully!"
echo " Binary:     ${BIN_DIR}/photodile"
echo " Desktop:    ${APP_DIR}/photodile.desktop"
echo " ServiceMenu: ${KIO_DIR}/photodile-folder.desktop"
echo "=========================================================="

