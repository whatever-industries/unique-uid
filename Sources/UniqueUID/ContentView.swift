import AppKit
import SwiftUI
import UIDCore
import UniformTypeIdentifiers

private let accent = Color(red: 0.29, green: 0.44, blue: 0.73)
private let accentHover = Color(red: 0.33, green: 0.47, blue: 0.76)
private let accentText = Color(red: 0.67, green: 0.77, blue: 0.97)
private let iconTint = Color(red: 0.82, green: 0.80, blue: 0.97)
private let surface = Color.white.opacity(0.04)
private let outline = Color.white.opacity(0.12)

struct ContentView: View {
    @State private var model = AppModel()
    @State private var showCopies = false
    @State private var isDropTargeted = false
    @State private var isDropHovered = false

    var body: some View {
        VStack(spacing: 10) {
            dropZone.disabled(model.activity != nil)
            copySettings.disabled(model.activity != nil)
            footer
        }
        .padding(.horizontal, 14)
        .padding(.top, 7)
        .padding(.bottom, 13)
        .frame(width: 410)
        .fixedSize(horizontal: false, vertical: true)
        .background {
            LinearGradient(
                colors: [Color(red: 0.112, green: 0.12, blue: 0.146), Color(red: 0.084, green: 0.090, blue: 0.108)],
                startPoint: .top, endPoint: .bottom
            )
            .ignoresSafeArea()
        }
        .tint(accent)
        .sheet(isPresented: $showCopies) { copiesSheet }
    }

    private var dropZone: some View {
        let highlighted = isDropTargeted || (isDropHovered && model.activity == nil)

        return Button(action: model.chooseSource) {
            HStack(spacing: 10) {
                Image(systemName: model.source == nil ? "arrow.down.document" : "doc.text")
                    .font(.system(size: 19, weight: .medium))
                    .foregroundStyle(iconTint)
                    .frame(width: 36, height: 36)
                    .background(accent.opacity(0.22), in: RoundedRectangle(cornerRadius: 8))
                    .accessibilityHidden(true)

                VStack(alignment: .leading, spacing: 2) {
                    if let source = model.source, let uid = model.sourceUID {
                        Text(source.lastPathComponent)
                            .font(.system(size: 13, weight: .semibold))
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .help(source.path)
                        Text(uid.description)
                            .font(.system(.caption, design: .monospaced))
                            .foregroundStyle(.secondary)
                            .accessibilityLabel("Source UID (uid.description)")
                    } else {
                        Text("Drop an Avid .avb or .avc file")
                            .font(.system(size: 13, weight: .semibold))
                    }
                }
                Spacer(minLength: 6)
                Text(model.source == nil ? "Browse…" : "Change…")
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(accentText)
            }
            .padding(10)
            .frame(minHeight: 59)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background(accent.opacity(highlighted ? 0.17 : 0.07), in: RoundedRectangle(cornerRadius: 10))
        .overlay {
            RoundedRectangle(cornerRadius: 10)
                .strokeBorder(
                    highlighted ? accentText : accentText.opacity(0.24),
                    style: StrokeStyle(lineWidth: 1, dash: model.source == nil ? [4, 3] : [])
                )
                .allowsHitTesting(false)
        }
        .onHover { isDropHovered = $0 }
        .onDrop(of: [.fileURL], isTargeted: $isDropTargeted, perform: receiveDrop)
    }

    private var copySettings: some View {
        @Bindable var bindable = model
        let count = Binding<Int>(
            get: { model.copyCount ?? 1 },
            set: { model.countText = String($0) }
        )

        return VStack(spacing: 5) {
            HStack(spacing: 6) {
                Label("Copies", systemImage: "square.on.square")
                Spacer()
                TextField("Copies", text: $bindable.countText)
                    .textFieldStyle(.roundedBorder)
                    .multilineTextAlignment(.trailing)
                    .monospacedDigit()
                    .frame(width: 48)
                    .accessibilityLabel("Number of copies")
                Stepper("Number of copies", value: count, in: 1...Int.max)
                    .labelsHidden()
            }
            Divider()
            HStack(spacing: 7) {
                Label("Save to", systemImage: "folder")
                Spacer(minLength: 7)
                Text(model.outputFolderOverride?.lastPathComponent ?? "Same as source")
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(model.outputFolder?.path ?? "Save beside the source file")
                if model.outputFolderOverride != nil {
                    Button("Use Source Folder", systemImage: "arrow.uturn.backward") {
                        model.outputFolderOverride = nil
                    }
                    .labelStyle(.iconOnly)
                    .help("Use Source Folder")
                }
                Button("Choose…", action: model.chooseOutputFolder)
                    .accessibilityLabel("Choose Output Folder")
            }
        }
        .controlSize(.small)
        .padding(.horizontal, 10)
        .padding(.vertical, 7)
        .background(surface, in: RoundedRectangle(cornerRadius: 9))
        .overlay {
            RoundedRectangle(cornerRadius: 9)
                .strokeBorder(outline, lineWidth: 1)
                .allowsHitTesting(false)
        }
    }

    private var footer: some View {
        VStack(spacing: 7) {
            Button(action: model.createCopies) {
                Label(model.copyCount == 1 ? "Create Copy" : "Create Copies", systemImage: "plus.square.on.square")
            }
            .buttonStyle(PrimaryButton())
            .disabled(model.source == nil || model.copyCount == nil || model.activity != nil)

            if let activity = model.activity {
                HStack(spacing: 6) {
                    ProgressView().controlSize(.small)
                    Text(activity).foregroundStyle(.secondary)
                }
            } else if !model.savedCopies.isEmpty {
                HStack(spacing: 7) {
                    Label(
                        model.savedCopies.count == 1 ? "1 copy saved" : "(model.savedCopies.count.formatted()) copies saved",
                        systemImage: "checkmark.circle"
                    )
                    .foregroundStyle(accentText)
                    Button("View UIDs") { showCopies = true }
                        .controlSize(.small)
                }
            } else if model.copyCount == nil {
                Text("Enter a positive whole number.").foregroundStyle(.secondary)
            }

            if let message = model.message {
                Label(message, systemImage: "exclamationmark.circle")
                    .foregroundStyle(.yellow)
                    .lineLimit(2)
                    .help(message)
            }
        }
    }

    private var copiesSheet: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Saved Copies").font(.title2.weight(.semibold))
            List(model.savedCopies) { copy in
                HStack(spacing: 12) {
                    Text(copy.url.lastPathComponent)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .help(copy.url.path)
                    Spacer()
                    Text(copy.uid.description)
                        .font(.system(.body, design: .monospaced))
                        .textSelection(.enabled)
                }
                .padding(.vertical, 4)
            }
            HStack {
                Button("Show in Finder") {
                    NSWorkspace.shared.activateFileViewerSelecting(model.savedCopies.map(\.url))
                }
                Spacer()
                Button("Done") { showCopies = false }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(18)
        .frame(width: 390, height: 240)
    }

    private func receiveDrop(_ providers: [NSItemProvider]) -> Bool {
        guard model.activity == nil else { return false }
        guard providers.count == 1, let provider = providers.first else {
            model.message = "Drop one bin or script at a time."
            return false
        }
        _ = provider.loadObject(ofClass: URL.self) { url, error in
            Task { @MainActor in
                guard model.activity == nil else { return }
                if let url {
                    model.loadSource(url)
                } else {
                    model.message = error?.localizedDescription ?? "Could not read the dropped file."
                }
            }
        }
        return true
    }
}

private struct PrimaryButton: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled
    @State private var isHovered = false

    func makeBody(configuration: Configuration) -> some View {
        let fill = isHovered && !configuration.isPressed ? accentHover : accent
        return configuration.label
            .font(.system(size: 14, weight: .semibold))
            .labelStyle(.titleAndIcon)
            .frame(width: 168, height: 35)
            .foregroundStyle(isEnabled ? .white : .secondary)
            .background {
                RoundedRectangle(cornerRadius: 9)
                    .fill(isEnabled ? fill : surface)
                    .overlay {
                        RoundedRectangle(cornerRadius: 9)
                            .fill(.linearGradient(
                                colors: [.white.opacity(isEnabled && !configuration.isPressed ? 0.12 : 0), .clear],
                                startPoint: .top, endPoint: .bottom
                            ))
                    }
                    .overlay {
                        RoundedRectangle(cornerRadius: 9)
                            .strokeBorder(.white.opacity(isEnabled ? 0.18 : 0.06), lineWidth: 1)
                    }
            }
            .onHover { isHovered = $0 }
    }
}
