#!/bin/sh
# Builds "Nai Ma Speak.app", a macOS app bundle around the companion, so it opens from Finder
# or the Dock without a Terminal window. Usage: ./macos-app.sh [destination folder]
# The app keeps config.json in the folder that holds the app.
set -eu
cd "$(dirname "$0")"
dest="${1:-target/release}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

# COMPANION_BINARY names a program built already, such as a universal one; without it
# the program is built here for this Mac.
binary="${COMPANION_BINARY:-}"
if [ -z "$binary" ]; then
    cargo build --release --locked
    binary=target/release/translate-companion
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
app="$work/Nai Ma Speak.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/translate-companion"

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
    <key>CFBundleName</key><string>Nai Ma Speak</string>
    <key>CFBundleDisplayName</key><string>Nai Ma Speak</string>
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

# macOS will not start an unsigned app on Apple silicon, and it ties the Screen Recording
# permission to the signature. With the certificate from macos-signing-cert.sh the
# signature stays the same from build to build, so the permission is granted once.
# Without it the app gets an ad-hoc signature and needs the permission again after
# every build (clear the old one with: tccutil reset ScreenCapture <bundle id>).
identity="${TRANSLATE_SIGN_IDENTITY:-Translate Local Signing}"
if security find-certificate -c "$identity" >/dev/null 2>&1; then
    codesign --force --sign "$identity" "$app" 2>/dev/null
    echo "Signed as \"$identity\""
else
    codesign --force --sign - "$app" 2>/dev/null
    echo "Signed ad hoc; run ./macos-signing-cert.sh once to keep permissions across builds"
fi

mkdir -p "$dest"
# Translate.app is what the app was called before; one left behind would be the old version.
rm -rf "$dest/Nai Ma Speak.app" "$dest/Translate.app"
mv "$app" "$dest/"
echo "Built $dest/Nai Ma Speak.app"
