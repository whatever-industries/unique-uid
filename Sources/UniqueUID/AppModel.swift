import AppKit
import Observation
import UIDCore
import UniformTypeIdentifiers

@MainActor
@Observable
final class AppModel {
    var source: URL?
    var sourceUID: AvidUID?
    var outputFolderOverride: URL?
    var countText = "1"
    var activity: String?
    var message: String?
    var savedCopies: [SavedCopy] = []
    private var issuedUIDs: Set<AvidUID> = []

    var outputFolder: URL? {
        outputFolderOverride ?? source?.deletingLastPathComponent()
    }

    var copyCount: Int? {
        guard let count = Int(countText.trimmingCharacters(in: .whitespaces)), count > 0 else { return nil }
        return count
    }

    func chooseSource() {
        let panel = NSOpenPanel()
        panel.title = "Choose Bin or Script"
        panel.prompt = "Choose"
        panel.canChooseDirectories = false
        panel.allowedContentTypes = ["avb", "avc"].compactMap { UTType(filenameExtension: $0) }
        panel.directoryURL = source?.deletingLastPathComponent()
        if panel.runModal() == .OK, let url = panel.url {
            loadSource(url)
        }
    }

    func loadSource(_ url: URL) {
        guard activity == nil else { return }
        message = nil
        savedCopies = []
        activity = "Reading file…"
        Task {
            do {
                let document = try await Task.detached(priority: .userInitiated) { try AvidDocument.read(from: url) }.value
                source = url
                sourceUID = document.uid
            } catch {
                message = error.localizedDescription
            }
            activity = nil
        }
    }

    func chooseOutputFolder() {
        let panel = NSOpenPanel()
        panel.title = "Choose Output Folder"
        panel.prompt = "Choose"
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.directoryURL = outputFolder
        if panel.runModal() == .OK, let url = panel.url {
            outputFolderOverride = url
        }
    }

    func createCopies() {
        guard activity == nil, let source, let outputFolder, let copyCount else { return }
        message = nil
        savedCopies = []
        activity = copyCount == 1 ? "Creating copy…" : "Creating copies…"
        let excluded = issuedUIDs
        Task {
            do {
                let batch = try await Task.detached(priority: .userInitiated) {
                    try AvidCopies.create(from: source, in: outputFolder, count: copyCount, excluding: excluded)
                }.value
                savedCopies = batch.copies
                issuedUIDs.formUnion(batch.copies.map(\.uid))
                message = batch.errorMessage
            } catch {
                message = error.localizedDescription
            }
            activity = nil
        }
    }
}
