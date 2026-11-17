#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

version="$(sed -nE 's/^version = "([0-9]+\.[0-9]+\.[0-9]+)"$/\1/p' Cargo.toml)"
tool_tag=1-alpha-20251107-1
tool_name=linuxdeploy-x86_64.AppImage
tool_dir="$(mktemp -d)"
trap 'find "$tool_dir" -depth -delete' EXIT

cargo build --locked --release --target-dir target

curl --fail --silent --show-error --location --retry 3 \
    "https://github.com/linuxdeploy/linuxdeploy/releases/download/$tool_tag/$tool_name" \
    -o "$tool_dir/$tool_name"
printf '%s  %s\n' 'c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d' \
    "$tool_dir/$tool_name" | sha256sum --check
chmod +x "$tool_dir/$tool_name"

mkdir -p dist "$tool_dir/AppDir/usr/share/licenses/unique-uid"
install -m 644 LICENSE assets/Inter-OFL.txt "$tool_dir/AppDir/usr/share/licenses/unique-uid/"
install -m 644 assets/icon.png "$tool_dir/unique-uid.png"
export APPIMAGE_EXTRACT_AND_RUN=1
export LDAI_OUTPUT="$PWD/dist/Unique.UID_Linux_x64_v$version.AppImage"
"$tool_dir/$tool_name" --appdir "$tool_dir/AppDir" \
    --executable target/release/unique-uid --desktop-file Scripts/unique-uid.desktop \
    --icon-file "$tool_dir/unique-uid.png" --output appimage
test -s "$LDAI_OUTPUT"
