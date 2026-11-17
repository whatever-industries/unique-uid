# Unique UID

A desktop utility for duplicating Avid Media Composer bins and scripts. Written in Rust.

[**Download Unique UID**](https://github.com/whatever-industries/unique-uid/releases/latest) for macOS 14 or newer (Apple Silicon or Intel), Windows (x64 or ARM64), or Linux (x64 or ARM64 AppImage).

![Unique UID window with the file drop, copy count, and output folder controls](docs/screenshot.png)

## Use

1. Drop one `.avb` or `.avc` file into the window, or click **Browse…**.
2. Set the copy count and choose an output folder. Copies save beside the source file by default.
3. Click **Create Copy** or **Create Copies**. Use **View UIDs** to see the assigned UIDs and open the output folder.

Copies receive random 64-bit UIDs that differ from the source and from other copies created in the current session. The app skips existing filenames. Only the eight-byte root UID changes, so clip and media MobIDs are preserved.

Supported layouts are AObjDoc container version 4, ABIN/BINF root versions 14 and 15, and Scpt root versions 2 and 3 with the version-1 rectangle. Opening an AVC copy alongside the original in Media Composer remains to be verified.

On Linux, drag-and-drop uses X11 or XWayland. On Wayland-only desktops, use **Browse…**. The Linux picker uses the XDG Desktop Portal or Zenity.

## Build and test

With Rust 1.92 or newer, run these from the project folder.

```sh
cargo test --locked
cargo run --locked
```

On Debian or Ubuntu, install the Wayland, XKB, and GL development packages first.

```sh
sudo apt-get install libwayland-dev libxkbcommon-dev libxkbcommon-x11-0 libgl1-mesa-dev libegl1-mesa-dev
```

On macOS, `./Scripts/build-app.sh` packages the app in `dist/`.

## Release

Update the version in `Cargo.toml` and `Cargo.lock`, update `RELEASE_NOTES.md`, and push a matching version tag. GitHub Actions tests and builds macOS ZIPs, Windows installers and Linux AppImages for both architectures, then publishes a release with a download table.

## License

[MIT](LICENSE). The bundled Inter font is licensed under the [SIL Open Font License](assets/Inter-OFL.txt).
