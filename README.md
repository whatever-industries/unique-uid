# Unique UID

A native macOS utility for duplicating Avid Media Composer bins and scripts.

[**Download Unique UID**](https://github.com/whatever-industries/unique-uid/releases/latest) for Apple Silicon or Intel. Requires macOS 14 or newer.

## Use

1. Drop one `.avb` or `.avc` file into the window, or click **Browse…**.
2. Enter the number of copies. Choose an output folder at any time, or use the default to save beside the source file.
3. Click **Create Copy** or **Create Copies**. Use **View UIDs** to see the assigned UIDs and locate the copies in Finder.

Copies receive random 64-bit UIDs that differ from the source and from other copies created in the current session. The app writes new files without overwriting existing names. Only the eight-byte root UID changes, so clip and media MobIDs are preserved.

Supported layouts are AObjDoc container version 4, ABIN/BINF root versions 14 and 15, and Scpt root versions 2 and 3 with the version-1 rectangle. Opening an AVC copy alongside the original in Media Composer remains to be verified.

## Build and test

With Swift 6 or newer, run these from the project folder.

```sh
swift test
./Scripts/build-app.sh
open "dist/Unique UID.app"
```

## Release

Update `RELEASE_NOTES.md` and push a version tag such as `v1.0.0`. GitHub Actions tests and builds both macOS downloads and publishes the release with links to each ZIP.

## License

[MIT](LICENSE)
