import Testing
@testable import Damon

private func event(_ type: String, _ payload: [String: JSONValue] = [:]) -> DamonEvent {
    DamonEvent(type: type, runId: "run", payload: payload, timestamp: "now", chatId: nil)
}

@Test func streamEventsDriveVisibleState() {
    var state = ConversationState()
    state.apply(event("ModelStarted"))
    state.apply(event("ReasoningDelta", ["delta": .string("think")]))
    state.apply(event("PythonDetected", ["source": .string("print(1)")]))
    state.apply(event("ExecutionStarted"))
    #expect(state.isRunActive)
    state.apply(event("StdoutDelta", ["delta": .string("1\n")]))
    state.apply(event("StderrDelta", ["delta": .string("warning")]))
    state.apply(event("ExecutionFinished", ["duration": .number(0.2), "exit_code": .number(0), "changed_files": .array([.string("created:result.txt")])]))
    state.apply(event("RunFinished"))
    #expect(!state.isRunActive)
    #expect(state.status == "Finished")
    #expect(state.reasoning == "think")
    #expect(state.python == "print(1)")
    #expect(state.stdout == "1\n")
    #expect(state.stderr == "warning")
    #expect(state.exitCode == 0)
    #expect(state.changedFiles == ["created:result.txt"])
    state.apply(event("RunStarted"))
    #expect(state.runId == "run")
}

@Test func thinkingEffortBelongsToModelSelection() {
    let plain = ZenModel(id: "plain", name: "Plain", reasoningEfforts: [])
    let reasoning = ZenModel(id: "reasoning", name: "Reasoning", reasoningEfforts: ["low", "high"])
    #expect(!plain.supportsThinkingEffort)
    #expect(reasoning.supportsThinkingEffort)
    #expect(reasoning.supports(effort: "high"))
    #expect(!reasoning.supports(effort: "max"))
}

@Test func ipcRejectsOverlongUnixSocketPath() async {
    let client = UnixSocketClient(path: String(repeating: "x", count: 200))
    await #expect(throws: IPCError.pathTooLong) {
        _ = try await client.request(IPCRequest(type: "ping"))
    }
}

private final class MemorySecrets: SecretStore {
    var value: String?
    func read() throws -> String? { value }
    func set(_ value: String) throws { self.value = value }
    func delete() throws { value = nil }
}

@MainActor @Test func settingsUseSecretStoreBoundary() throws {
    let secrets = MemorySecrets()
    let settings = SettingsModel(secrets: secrets)
    settings.apiKey = "secret"
    try settings.saveKey()
    #expect(secrets.value == "secret")
    try settings.deleteKey()
    #expect(secrets.value == nil)
    #expect(settings.apiKey.isEmpty)
}

@Test func keychainDefaultsStayOnProductionServiceBoundary() {
    #expect(KeychainStore(service: "test-service").service == "test-service")
    #expect(KeychainStore(service: "test-service").account == "opencode-zen-api-key")
}

@MainActor @Test func defaultPromptDefinesTheDeterministicResponseEnvelope() {
    #expect(SettingsModel.defaultSystemPrompt.contains("ACTION: python"))
    #expect(SettingsModel.defaultSystemPrompt.contains("ACTION: finish"))
    #expect(SettingsModel.defaultSystemPrompt.contains("exactly one fenced Python program"))
}
