import Foundation

struct ConversationState: Equatable, Sendable {
    var answer = ""
    var reasoning = ""
    var python = ""
    var stdout = ""
    var stderr = ""
    var status = "Ready"
    var duration: Double?
    var exitCode: Int?
    var scriptPath: String?
    var runId: String?
    var changedFiles: [String] = []
    var error: String?
    var isRunActive: Bool { ["Generating", "Executing Python", "Continuing"].contains(status) }

    mutating func apply(_ event: DamonEvent) {
        switch event.type {
        case "RunStarted": runId = event.runId
        case "ModelStarted": status = "Generating"; answer = ""
        case "ReasoningDelta": reasoning += event.payload["delta"]?.string ?? ""
        case "TextDelta": answer += event.payload["delta"]?.string ?? ""
        case "PythonDetected": python = event.payload["source"]?.string ?? ""; answer = ""
        case "ScriptSaved": scriptPath = event.payload["path"]?.string
        case "ExecutionStarted": status = "Executing Python"
        case "StdoutDelta": stdout += event.payload["delta"]?.string ?? ""
        case "StderrDelta": stderr += event.payload["delta"]?.string ?? ""
        case "ExecutionFinished":
            status = "Continuing"
            duration = event.payload["duration"]?.number
            exitCode = event.payload["exit_code"]?.number.map(Int.init)
            changedFiles = event.payload["changed_files"]?.strings ?? []
        case "RunFinished": status = "Finished"; answer = event.payload["answer"]?.string ?? answer
        case "Error":
            status = "Error"
            error = event.payload["message"]?.string
        default: break
        }
    }
}

@MainActor
final class AppModel: ObservableObject {
    @Published var conversation = ConversationState()
    @Published var input = ""
    @Published var chats: [ChatRecord] = []
    @Published var tools: [ToolRecord] = []
    @Published var isActivityVisible = true
    @Published var isConnected = false
    @Published var models: [ZenModel] = []
    @Published var selectedModel = UserDefaults.standard.string(forKey: "selectedModel") ?? ""
    @Published var sidebarSelection = "conversation"
    @Published var messages: [MessageRecord] = []
    @Published var activeChatId: String?
    private let daemon = DaemonManager()
    private lazy var client = UnixSocketClient(path: daemon.socketPath)

    func start() {
        conversation.status = "Connecting"
        Task { @MainActor in
            if (try? await client.request(IPCRequest(type: "ping"))) == nil {
                do { try daemon.start() }
                catch { conversation.status = "Daemon unavailable"; conversation.error = String(describing: error); return }
            }
            for attempt in 0..<30 {
                do {
                    _ = try await client.request(IPCRequest(type: "ping"))
                    isConnected = true
                    try await configureAndLoad()
                    return
                } catch {
                    if attempt == 29 { conversation.status = "Daemon disconnected"; conversation.error = String(describing: error) }
                    try? await Task.sleep(for: .milliseconds(100))
                }
            }
        }
    }

    func stop() { daemon.stop() }

    private func configureAndLoad() async throws {
        let toolLines = try await client.request(IPCRequest(type: "tools"))
        tools = try JSONDecoder().decode(IPCResponse.self, from: toolLines[0]).tools ?? []
        let chatLines = try await client.request(IPCRequest(type: "chats"))
        chats = try JSONDecoder().decode(IPCResponse.self, from: chatLines[0]).chats ?? []
        let key = await KeychainStore().readWithTimeout()
        guard let key, !key.isEmpty else {
            conversation.status = "API key required"
            return
        }
        _ = try await client.request(IPCRequest(type: "configure", apiKey: key))
        let modelLines = try await client.request(IPCRequest(type: "models"))
        let response = try JSONDecoder().decode(IPCResponse.self, from: modelLines[0])
        models = response.models ?? []
        if !models.contains(where: { $0.id == selectedModel }) { selectedModel = models.first?.id ?? "" }
        UserDefaults.standard.set(selectedModel, forKey: "selectedModel")
        conversation.status = "Ready"
    }

    func reloadConfiguration() {
        conversation.status = "Connecting"
        Task { @MainActor in
            do { try await configureAndLoad() }
            catch { conversation.status = "Connection failed"; conversation.error = String(describing: error) }
        }
    }

    func configure(apiKey: String) {
        conversation.status = "Connecting"
        Task { @MainActor in
            do {
                _ = try await client.request(IPCRequest(type: "configure", apiKey: apiKey))
                let modelLines = try await client.request(IPCRequest(type: "models"))
                models = try JSONDecoder().decode(IPCResponse.self, from: modelLines[0]).models ?? []
                if !models.contains(where: { $0.id == selectedModel }) { selectedModel = models.first?.id ?? "" }
                conversation.status = "Ready"
            } catch { conversation.status = "Connection failed"; conversation.error = String(describing: error) }
        }
    }

    func promoteCurrentScript() {
        guard let path = conversation.scriptPath else { return }
        let runId = conversation.runId ?? "unknown"
        let baseName = URL(fileURLWithPath: path).deletingPathExtension().lastPathComponent
        let toolName = "\(runId.prefix(8))-\(baseName)"
        Task { @MainActor in
            do {
                _ = try await client.request(IPCRequest(type: "promote", model: selectedModel, path: path, name: toolName, description: "Saved from a Damon run", category: "misc", runId: runId))
                let lines = try await client.request(IPCRequest(type: "tools"))
                tools = try JSONDecoder().decode(IPCResponse.self, from: lines[0]).tools ?? []
                sidebarSelection = "tools"
            } catch { conversation.error = String(describing: error) }
        }
    }

    func cancelRun() {
        Task { @MainActor in
            do { _ = try await client.request(IPCRequest(type: "cancel")) }
            catch { conversation.error = String(describing: error) }
        }
    }

    func clearAPIKey() {
        Task { @MainActor in
            do {
                _ = try await client.request(IPCRequest(type: "clear_api_key"))
                models = []
                selectedModel = ""
                conversation.status = "API key required"
            } catch { conversation.error = String(describing: error) }
        }
    }

    func selectSidebar(_ selection: String) {
        guard selection.hasPrefix("chat:") else { return }
        let chatId = String(selection.dropFirst(5))
        activeChatId = chatId
        Task { @MainActor in
            do {
                let lines = try await client.request(IPCRequest(type: "messages", chatId: chatId))
                messages = try JSONDecoder().decode(IPCResponse.self, from: lines[0]).messages ?? []
                conversation = ConversationState()
            } catch { conversation.error = String(describing: error) }
        }
    }

    func send() {
        let message = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !message.isEmpty, !selectedModel.isEmpty, !conversation.isRunActive else { return }
        input = ""
        conversation = ConversationState(status: "Generating")
        messages.append(MessageRecord(role: "user", content: message, createdAt: ""))
        let defaults = UserDefaults.standard
        Task { @MainActor in
            do {
                let maxTurns = defaults.integer(forKey: "maxTurns")
                let timeout = defaults.double(forKey: "executionTimeout")
                let lines = try client.stream(IPCRequest(
                    type: "run", message: message, model: selectedModel,
                    thinkingEffort: defaults.string(forKey: "thinkingEffort"),
                    systemPrompt: defaults.string(forKey: "systemPrompt"),
                    workingDirectory: defaults.string(forKey: "workingDirectory") ?? FileManager.default.homeDirectoryForCurrentUser.path,
                    pythonExecutable: defaults.string(forKey: "pythonExecutable") ?? "python3",
                    maxTurns: maxTurns == 0 ? 8 : maxTurns,
                    executionTimeout: timeout == 0 ? 60 : timeout
                    , chatId: activeChatId,
                    rawEventLogging: defaults.object(forKey: "rawEventLogging") == nil ? true : defaults.bool(forKey: "rawEventLogging")
                ))
                for try await line in lines {
                    if let event = try? JSONDecoder().decode(DamonEvent.self, from: line) {
                        if event.type == "RunStarted" { activeChatId = event.chatId }
                        receive(event)
                    }
                    else if let error = try? JSONDecoder().decode(IPCResponse.self, from: line), error.type == "error" {
                        conversation.status = "Error"; conversation.error = error.message
                    }
                }
                if let chatId = activeChatId {
                    let messageLines = try await client.request(IPCRequest(type: "messages", chatId: chatId))
                    messages = try JSONDecoder().decode(IPCResponse.self, from: messageLines[0]).messages ?? messages
                }
                let chatLines = try await client.request(IPCRequest(type: "chats"))
                chats = try JSONDecoder().decode(IPCResponse.self, from: chatLines[0]).chats ?? chats
            } catch { conversation.status = "Daemon disconnected"; conversation.error = String(describing: error) }
        }
    }

    func receive(_ event: DamonEvent) { conversation.apply(event) }
    func newChat() { conversation = ConversationState(); input = ""; messages = []; activeChatId = nil }
}

struct ToolRecord: Identifiable, Codable, Equatable, Sendable {
    let id: String
    let name: String
    let description: String
    let path: String
    let category: String
}

struct ChatRecord: Identifiable, Codable, Equatable, Sendable {
    let id: String
    let title: String
    let createdAt: String
    enum CodingKeys: String, CodingKey { case id, title; case createdAt = "created_at" }
}

struct MessageRecord: Codable, Equatable, Sendable {
    let role: String
    let content: String
    let createdAt: String
    enum CodingKeys: String, CodingKey { case role, content; case createdAt = "created_at" }
}
