#!/usr/bin/env bash
# make-icon.sh — Build the "Verify & Launch Codex" .icns icon
#
# Extracts the Codex app icon, composites a dark-blue shield-with-checkmark
# badge in the bottom-right corner, then produces a full macOS iconset (.icns).
#
# Requirements: sips, iconutil (both macOS built-ins), magick (ImageMagick 7)
# Usage: ./scripts/make-icon.sh <output.icns> [/path/to/Codex.app]

set -euo pipefail

OUTPUT="${1:?Usage: make-icon.sh <output.icns> [Codex.app path]}"
CODEX_APP="${2:-/Applications/Codex.app}"
CODEX_ICNS="$CODEX_APP/Contents/Resources/icon.icns"

if [[ ! -f "$CODEX_ICNS" ]]; then
    echo "✗ Cannot find Codex icon at $CODEX_ICNS" >&2
    exit 1
fi

WORK=$(mktemp -d)
trap "rm -rf '$WORK'" EXIT

# ---------------------------------------------------------------------------
# 1. Extract 512×512 base PNG from Codex.app icon
# ---------------------------------------------------------------------------
sips -s format png -z 512 512 "$CODEX_ICNS" --out "$WORK/base.png" >/dev/null

# ---------------------------------------------------------------------------
# 2. Render shield badge (180×180 canvas, dark-blue circle + shield + ✓)
# ---------------------------------------------------------------------------
magick -size 180x180 xc:none \
    `# Outer dark-blue circle` \
    -fill "#1e40af" -draw "circle 90,90 90,4" \
    `# White shield (outer)` \
    -fill white \
    -draw "polygon 90,18 155,46 155,100 90,168 25,100 25,46" \
    `# Dark-blue shield (inner — creates border effect)` \
    -fill "#1e40af" \
    -draw "polygon 90,38 140,60 140,96 90,148 40,96 40,60" \
    `# White checkmark — two thick strokes` \
    -strokewidth 9 -stroke white -fill none \
    -draw "line 62,92 78,112" \
    -draw "line 78,112 122,72" \
    "$WORK/badge.png"

# ---------------------------------------------------------------------------
# 3. Composite badge onto base icon — bottom-right, 14 px inset
# ---------------------------------------------------------------------------
magick "$WORK/base.png" \
    \( "$WORK/badge.png" -resize 150x150 \) \
    -gravity SouthEast -geometry +14+14 \
    -composite \
    "$WORK/badged.png"

# ---------------------------------------------------------------------------
# 4. Build macOS iconset (all required sizes + @2x variants)
# ---------------------------------------------------------------------------
ICONSET="$WORK/AppIcon.iconset"
mkdir -p "$ICONSET"

for size in 16 32 128 256 512; do
    magick "$WORK/badged.png" -resize "${size}x${size}" \
        "$ICONSET/icon_${size}x${size}.png"
    double=$((size * 2))
    magick "$WORK/badged.png" -resize "${double}x${double}" \
        "$ICONSET/icon_${size}x${size}@2x.png"
done

# ---------------------------------------------------------------------------
# 5. Compile iconset → .icns
# ---------------------------------------------------------------------------
iconutil -c icns -o "$OUTPUT" "$ICONSET"
echo "→ Icon written to $OUTPUT"
