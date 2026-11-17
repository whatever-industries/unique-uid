// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "UniqueUID",
    platforms: [.macOS(.v14)],
    targets: [
        .target(name: "UIDCore"),
        .executableTarget(name: "UniqueUID", dependencies: ["UIDCore"]),
        .testTarget(name: "UIDCoreTests", dependencies: ["UIDCore"])
    ]
)
