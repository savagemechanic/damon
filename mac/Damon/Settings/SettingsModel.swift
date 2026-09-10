import SwiftUI

@MainActor
final class SettingsModel: ObservableObject {
    @AppStorage("selectedModel") var selectedModel = ""
    @AppStorage("thinkingEffort") var thinkingEffort = ""
    @AppStorage("systemPrompt") var systemPrompt = "You are the reasoning engine inside Damon. Inspect before modifying; emit minimal Python actions and finish only with evidence."
    @AppStorage("pythonExecutable") var pythonExecutable = "python3"
    @AppStorage("maxTurns") var maxTurns = 8
    @AppStorage("executionTimeout") var executionTimeout = 60.0
    @AppStorage("workingDirectory") var workingDirectory = FileManager.default.homeDirectoryForCurrentUser.path
    @AppStorage("toolsDirectory") var toolsDirectory = FileManager.default.homeDirectoryForCurrentUser.appending(path: ".damon/tools").path
    @AppStorage("reasoningVisibility") var reasoningVisibility = true
    @AppStorage("rawEventLogging") var rawEventLogging = true
    @Published var apiKey = ""
    @Published var models: [String] = []
    @Published var effortOptions: [String] = []
    @Published var connectionMessage = "Not tested"
    private let secrets: SecretStore

    init(secrets: SecretStore = KeychainStore()) {
        self.secrets = secrets
        if let store = secrets as? KeychainStore {
            Task { apiKey = await store.readWithTimeout() ?? "" }
        } else {
            apiKey = (try? secrets.read()) ?? ""
        }
    }

    var thinkingEnabled: Bool { !effortOptions.isEmpty }
    func saveKey() throws { try secrets.set(apiKey) }
    func deleteKey() throws { try secrets.delete(); apiKey = "" }
}
