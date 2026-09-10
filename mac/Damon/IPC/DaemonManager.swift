import Foundation

@MainActor
final class DaemonManager {
    private var process: Process?
    private let homePath = ProcessInfo.processInfo.environment["DAMON_HOME"] ?? FileManager.default.homeDirectoryForCurrentUser.appending(path: ".damon").path
    var socketPath: String { URL(fileURLWithPath: homePath).appending(path: "damon.sock").path }

    func start() throws {
        if process?.isRunning == true { return }
        guard let resources = Bundle.main.resourceURL else { throw IPCError.disconnected }
        let source = resources.appending(path: "python/src").path
        guard FileManager.default.fileExists(atPath: source) else { throw IPCError.disconnected }
        let task = Process()
        let frameworks = resources.appending(path: "Frameworks")
        let versions = frameworks.appending(path: "Python.framework/Versions")
        let versionDirectory = (try? FileManager.default.contentsOfDirectory(at: versions, includingPropertiesForKeys: nil))?.first(where: { $0.lastPathComponent != "Current" })
        let binaries = versionDirectory?.appending(path: "bin")
        let bundled = binaries.flatMap { try? FileManager.default.contentsOfDirectory(at: $0, includingPropertiesForKeys: nil) }?
            .first(where: { $0.lastPathComponent.hasPrefix("python3.") && !$0.lastPathComponent.hasSuffix("-config") && FileManager.default.isExecutableFile(atPath: $0.path) })?.path
        let candidates = [bundled, "/opt/homebrew/bin/python3", "/usr/local/bin/python3", "/usr/bin/python3"].compactMap { $0 }
        guard let python = candidates.first(where: { FileManager.default.isExecutableFile(atPath: $0) }) else { throw IPCError.disconnected }
        task.executableURL = URL(fileURLWithPath: python)
        task.arguments = ["-m", "damon.ipc.server", "--socket", socketPath, "--home", homePath]
        var environment = ProcessInfo.processInfo.environment
        environment["PYTHONPATH"] = source
        environment["PYTHONDONTWRITEBYTECODE"] = "1"
        if bundled != nil { environment["DYLD_FRAMEWORK_PATH"] = frameworks.path }
        task.environment = environment
        task.standardOutput = FileHandle.nullDevice
        let logURL = URL(fileURLWithPath: homePath).appending(path: "daemon.log")
        try FileManager.default.createDirectory(at: logURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        if !FileManager.default.fileExists(atPath: logURL.path) { FileManager.default.createFile(atPath: logURL.path, contents: nil) }
        task.standardError = try FileHandle(forWritingTo: logURL)
        try task.run()
        process = task
    }

    func stop() { process?.terminate(); process = nil }
}
