import SwiftUI

@MainActor
final class SettingsModel: ObservableObject {
    static let defaultSystemPrompt = """
    You are the reasoning engine inside Damon. Interact with the local computer by returning either ACTION: python with exactly one fenced Python program, or ACTION: finish with the final answer. Damon executes generated code and returns real results. Inspect before modifying. Prefer minimal deterministic standard-library Python. Never claim an action succeeded without evidence. Finish only when sufficient evidence exists.
    """

    @AppStorage("selectedModel") var selectedModel = ""
    @AppStorage("thinkingEffort") var thinkingEffort = ""
    @AppStorage("systemPrompt") var systemPrompt = SettingsModel.defaultSystemPrompt
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
