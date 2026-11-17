#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

version="$(sed -nE 's/^version = "([0-9]+\.[0-9]+\.[0-9]+)"$/\1/p' Cargo.toml)"
tool_tag=1-alpha-20251107-1
architecture="$(uname -m)"
case "$architecture" in
    x86_64)
        platform=Linux_x64
        tool_digest=c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d
        ;;
    aarch64)
        platform=Linux_ARM
        tool_digest=620095110d693282b8ebeb244a95b5e911cf8f65f76c88b4b47d16ae6346fcff
        ;;
    *)
        printf 'Unsupported AppImage architecture: %s\n' "$architecture" >&2
        exit 1
        ;;
esac
tool_name="linuxdeploy-$architecture.AppImage"
tool_dir="$(mktemp -d)"
trap 'find "$tool_dir" -depth -delete' EXIT

cargo build --locked --release --target-dir target

curl --fail --silent --show-error --location --retry 3 \
    "https://github.com/linuxdeploy/linuxdeploy/releases/download/$tool_tag/$tool_name" \
    -o "$tool_dir/$tool_name"
printf '%s  %s\n' "$tool_digest" \
    "$tool_dir/$tool_name" | sha256sum --check
chmod +x "$tool_dir/$tool_name"

mkdir -p dist "$tool_dir/AppDir/usr/share/licenses/unique-uid"
install -m 644 LICENSE assets/Inter-OFL.txt "$tool_dir/AppDir/usr/share/licenses/unique-uid/"
install -m 644 assets/icon.png "$tool_dir/unique-uid.png"
# Winit opens this library dynamically, so dependency scanning cannot see it.
xkb_x11="$(ldconfig -p | awk '$1 == "libxkbcommon-x11.so.0" && !found++ {print $NF}')"
if [[ -z "$xkb_x11" ]]; then
    echo "Install libxkbcommon-x11 before packaging the AppImage." >&2
    exit 1
fi
export APPIMAGE_EXTRACT_AND_RUN=1
export LDAI_OUTPUT="$PWD/dist/Unique.UID_${platform}_v$version.AppImage"
"$tool_dir/$tool_name" --appdir "$tool_dir/AppDir" \
    --executable target/release/unique-uid --desktop-file Scripts/unique-uid.desktop \
    --library "$xkb_x11" --icon-file "$tool_dir/unique-uid.png" --output appimage
test -s "$LDAI_OUTPUT"
