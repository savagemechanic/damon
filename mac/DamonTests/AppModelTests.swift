import Testing
@testable import Damon

private func event(_ type: String, _ payload: [String: JSONValue] = [:]) -> DamonEvent {
    DamonEvent(type: type, runId: "run", payload: payload, timestamp: "now")
}

@Test func streamEventsDriveVisibleState() {
    var state = ConversationState()
    state.apply(event("ModelStarted"))
    state.apply(event("ReasoningDelta", ["delta": .string("think")]))
    state.apply(event("PythonDetected", ["source": .string("print(1)")]))
    state.apply(event("ExecutionStarted"))
    state.apply(event("StdoutDelta", ["delta": .string("1\n")]))
    state.apply(event("StderrDelta", ["delta": .string("warning")]))
    state.apply(event("ExecutionFinished", ["duration": .number(0.2), "exit_code": .number(0)]))
    state.apply(event("RunFinished"))
    #expect(state.status == "Finished")
    #expect(state.reasoning == "think")
    #expect(state.python == "print(1)")
    #expect(state.stdout == "1\n")
    #expect(state.stderr == "warning")
    #expect(state.exitCode == 0)
}

@Test func thinkingEffortBelongsToModelSelection() {
    var selection = ModelSelection(modelId: "plain")
    #expect(!selection.supportsThinkingEffort)
    selection.effortOptions = ["low", "high"]
    #expect(selection.supportsThinkingEffort)
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
