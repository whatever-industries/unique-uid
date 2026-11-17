### Windows and Linux

Unique UID now runs on Windows and Linux as well as macOS. The Rust app keeps the compact file drop, copy count and output folder picker, and adds a new app icon.

Windows and Linux downloads target x64. The Linux AppImage can launch with `--appimage-extract-and-run` on systems without FUSE. macOS builds require macOS 14 or newer and are ad hoc signed. [Apple’s first-launch instructions](https://support.apple.com/en-us/102445)

Opening an original and copied `.avc` together in Media Composer still needs a live test.
