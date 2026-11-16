#!/bin/sh
# Builds Translate.app, a macOS app bundle around the companion, so it opens from Finder
# or the Dock without a Terminal window. Usage: ./macos-app.sh [destination folder]
# The app keeps config.json in the folder that holds Translate.app.
set -eu
cd "$(dirname "$0")"
dest="${1:-target/release}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

cargo build --release --locked

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
app="$work/Translate.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/translate-companion "$app/Contents/MacOS/"

icons="$work/icon.iconset"
mkdir "$icons"
for size in 16 32 128 256; do
    sips -z "$size" "$size" ../assets/icon-512.png --out "$icons/icon_${size}x${size}.png" >/dev/null
    sips -z "$((size * 2))" "$((size * 2))" ../assets/icon-512.png --out "$icons/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$icons" -o "$app/Contents/Resources/icon.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Translate</string>
    <key>CFBundleDisplayName</key><string>Translate</string>
    <key>CFBundleIdentifier</key><string>com.github.norswap.translate-companion</string>
    <key>CFBundleExecutable</key><string>translate-companion</string>
    <key>CFBundleIconFile</key><string>icon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# An ad-hoc signature: macOS will not start an unsigned app on Apple silicon.
codesign --force --sign - "$app" 2>/dev/null

mkdir -p "$dest"
rm -rf "$dest/Translate.app"
mv "$app" "$dest/"
echo "Built $dest/Translate.app"
