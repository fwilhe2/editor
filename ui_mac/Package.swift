// swift-tools-version: 5.9
//
// Deliberately Swift Package Manager rather than a hand-written .xcodeproj: a
// .pbxproj is machine-generated, nearly unreviewable and easy to corrupt. Xcode
// opens this package directly, and `xcodebuild` builds it, so nothing is lost.
//
// Language mode 5 on purpose — Swift 6's strict concurrency checking rejects the
// generated bindings' use of `@unchecked Sendable`.

import PackageDescription

let package = Package(
    name: "EditorApp",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "EditorApp", targets: ["EditorApp"]),
        .executable(name: "FfiSmoke", targets: ["FfiSmoke"]),
    ],
    targets: [
        // The C header and module map for the Rust library.
        .target(name: "EditorFFI"),

        // The UniFFI-generated Swift API.
        .target(name: "EditorCore", dependencies: ["EditorFFI"]),

        // The SwiftUI app. macOS only.
        .executableTarget(name: "EditorApp", dependencies: ["EditorCore"]),

        // Exercises the FFI boundary with no UI, so it also runs on Linux:
        //   swift build --package-path ui_mac --target FfiSmoke
        .executableTarget(name: "FfiSmoke", dependencies: ["EditorCore"]),
    ]
)
