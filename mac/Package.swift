// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "Damon",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "Damon", targets: ["Damon"])],
    targets: [
        .executableTarget(name: "Damon", path: "Damon", exclude: ["Info.plist"]),
        .testTarget(name: "DamonTests", dependencies: ["Damon"], path: "DamonTests"),
    ]
)
