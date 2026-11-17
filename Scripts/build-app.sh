#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."

version="$(sed -nE 's/^version = "([0-9]+\.[0-9]+\.[0-9]+)"$/\1/p' Cargo.toml)"
app="dist/Unique UID.app"
target=${1:-}
compiler_flags=()
bin_dir=target/release
if [[ -n "$target" ]]; then
    compiler_flags=(--target "$target")
    bin_dir="target/$target/release"
fi
# Preserve proc-macro line tables for Xcode 27's stricter dylib loader.
MACOSX_DEPLOYMENT_TARGET=14.0 CARGO_PROFILE_RELEASE_DEBUG=${CARGO_PROFILE_RELEASE_DEBUG:-1} \
    cargo build --locked --release --target-dir target ${compiler_flags[@]+"${compiler_flags[@]}"}

resources="$app/Contents/Resources"
mkdir -p "$app/Contents/MacOS" "$resources"
install -m 755 "$bin_dir/unique-uid" "$app/Contents/MacOS/UniqueUID"
install -m 644 LICENSE assets/Inter-OFL.txt "$resources/"

icon_dir="$(mktemp -d)"
trap 'find "$icon_dir" -depth -delete' EXIT
mkdir "$icon_dir/UniqueUID.iconset"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" assets/icon.png --out "$icon_dir/UniqueUID.iconset/icon_${size}x${size}.png" >/dev/null
    sips -z "$((2 * size))" "$((2 * size))" assets/icon.png --out "$icon_dir/UniqueUID.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns -o "$resources/UniqueUID.icns" "$icon_dir/UniqueUID.iconset"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>Unique UID</string>
    <key>CFBundleExecutable</key>
    <string>UniqueUID</string>
    <key>CFBundleIconFile</key>
    <string>UniqueUID</string>
    <key>CFBundleIdentifier</key>
    <string>local.uniqueuid.app</string>
    <key>CFBundleName</key>
    <string>Unique UID</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$version</string>
    <key>CFBundleVersion</key>
    <string>$version</string>
    <key>LSMinimumSystemVersion</key>
    <string>14.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST
codesign --force --sign - "$app"
printf '%s/%s\n' "$PWD" "$app"
