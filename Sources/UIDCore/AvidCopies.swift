import Darwin
import Foundation

public struct SavedCopy: Sendable, Identifiable {
    public let url: URL
    public let uid: AvidUID

    public var id: URL { url }
}

public struct CopyBatch: Sendable {
    public let copies: [SavedCopy]
    public let errorMessage: String?
}

public enum AvidCopies {
    public static func create(
        from source: URL,
        in destination: URL,
        count: Int,
        excluding excludedUIDs: Set<AvidUID>
    ) throws -> CopyBatch {
        guard count > 0 else { throw CopyError.invalidCount }
        let snapshot = try AvidDocument.snapshot(from: source)
        let document = snapshot.document
        var data = snapshot.data
        var usedUIDs = excludedUIDs
        usedUIDs.insert(document.uid)
        usedUIDs.insert(AvidUID(0))

        let manager = FileManager.default
        let stagingFolder = destination.appendingPathComponent(".unique-uid-" + UUID().uuidString, isDirectory: true)
        try manager.createDirectory(at: stagingFolder, withIntermediateDirectories: false)
        defer { try? manager.removeItem(at: stagingFolder) }

        let stagedFile = stagingFolder.appendingPathComponent("copy")
        let originalStem = source.deletingPathExtension().lastPathComponent
        let ext = source.pathExtension
        var nameIndex = 1
        var copies: [SavedCopy] = []

        do {
            for _ in 0..<count {
                var uid: AvidUID
                repeat {
                    uid = AvidUID(UInt64.random(in: UInt64.min...UInt64.max))
                } while !usedUIDs.insert(uid).inserted

                document.replaceUID(in: &data, with: uid)
                try data.write(to: stagedFile)

                while true {
                    let suffix = (nameIndex == 1 ? " Copy" : " Copy \(nameIndex)") + "." + ext
                    var stem = originalStem
                    while stem.utf8.count + suffix.utf8.count > 255 { stem.removeLast() }
                    let target = destination.appendingPathComponent(stem + suffix)
                    nameIndex += 1
                    if try publish(stagedFile, at: target) {
                        copies.append(SavedCopy(url: target, uid: uid))
                        break
                    }
                }
            }
            return CopyBatch(copies: copies, errorMessage: nil)
        } catch {
            return CopyBatch(copies: copies, errorMessage: error.localizedDescription)
        }
    }

    private static func publish(_ stagedFile: URL, at destination: URL) throws -> Bool {
        try stagedFile.withUnsafeFileSystemRepresentation { sourcePath in
            try destination.withUnsafeFileSystemRepresentation { destinationPath in
                guard let sourcePath, let destinationPath else { throw CopyError.invalidPath }
                // Publish a complete file, and claim the name atomically even across app instances.
                if renamex_np(sourcePath, destinationPath, UInt32(RENAME_EXCL)) == 0 { return true }
                let code = errno
                if code == EEXIST { return false }
                throw NSError(
                    domain: NSPOSIXErrorDomain,
                    code: Int(code),
                    userInfo: [NSFilePathErrorKey: destination.path]
                )
            }
        }
    }

    private enum CopyError: LocalizedError {
        case invalidCount, invalidPath

        var errorDescription: String? {
            switch self {
            case .invalidCount: "Enter a copy count of at least 1."
            case .invalidPath: "Could not use the destination path."
            }
        }
    }
}
