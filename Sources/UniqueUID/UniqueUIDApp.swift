import SwiftUI

@main
struct UniqueUIDApp: App {
    var body: some Scene {
        Window("Unique UID", id: "randomizer") {
            ContentView()
                .preferredColorScheme(.dark)
        }
        .windowResizability(.contentSize)
        .windowStyle(.hiddenTitleBar)
    }
}
