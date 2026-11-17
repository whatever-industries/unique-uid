#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."

version="${1:-1.0.0}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    printf 'Expected a version such as 1.0.0, got %s\n' "$version" >&2
    exit 1
fi

swift build --configuration release --product UniqueUID
bin_dir="$(swift build --configuration release --show-bin-path)"
app="dist/Unique UID.app"

mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 644 LICENSE "$app/Contents/Resources/LICENSE"
install -m 755 "$bin_dir/UniqueUID" "$app/Contents/MacOS/UniqueUID"
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
