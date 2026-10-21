#!/usr/bin/env bash
set -e

# Photodile - Linux User Uninstallation Script

BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"
ICON_BASE="${HOME}/.local/share/icons/hicolor"
PIXMAP_DIR="${HOME}/.local/share/pixmaps"
KIO_DIR="${HOME}/.local/share/kio/servicemenus"
MIMEAPPS="${HOME}/.config/mimeapps.list"

echo "==> Removing Photodile binary and desktop integrations..."

rm -f "${BIN_DIR}/photodile"
rm -f "${APP_DIR}/photodile.desktop"
rm -f "${KIO_DIR}/photodile-folder.desktop"
rm -f "${PIXMAP_DIR}/photodile.png"

for size in 16 32 48 64 128 256 512; do
    rm -f "${ICON_BASE}/${size}x${size}/apps/photodile.png"
done

if [ -f "${MIMEAPPS}" ]; then
    echo "==> Cleaning MIME associations..."
    sed -i '/=photodile\.desktop/d' "${MIMEAPPS}"
    sed -i 's/photodile\.desktop;//g' "${MIMEAPPS}"
fi

echo "==> Updating desktop and icon caches..."
update-desktop-database "${APP_DIR}" 2>/dev/null || true
gtk-update-icon-cache -f -t "${ICON_BASE}" 2>/dev/null || true
kbuildsycoca6 2>/dev/null || true

echo "Photodile uninstalled successfully."

