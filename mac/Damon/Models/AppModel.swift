import Foundation

struct ModelSelection: Equatable, Sendable {
    var provider = "OpenCode Zen"
    var modelId = ""
    var thinkingEffort: String?
    var effortOptions: [String] = []
    var supportsThinkingEffort: Bool { !effortOptions.isEmpty }
}

struct ConversationState: Equatable, Sendable {
    var answer = ""
    var reasoning = ""
    var python = ""
    var stdout = ""
    var stderr = ""
    var status = "Ready"
    var duration: Double?
    var exitCode: Int?
    var error: String?

    mutating func apply(_ event: DamonEvent) {
        switch event.type {
        case "ModelStarted": status = "Generating"
        case "ReasoningDelta": reasoning += event.payload["delta"]?.string ?? ""
        case "TextDelta": answer += event.payload["delta"]?.string ?? ""
        case "PythonDetected": python = event.payload["source"]?.string ?? ""
        case "ExecutionStarted": status = "Executing Python"
        case "StdoutDelta": stdout += event.payload["delta"]?.string ?? ""
        case "StderrDelta": stderr += event.payload["delta"]?.string ?? ""
        case "ExecutionFinished":
            status = "Continuing"
            duration = event.payload["duration"]?.number
            exitCode = event.payload["exit_code"]?.number.map(Int.init)
        case "RunFinished": status = "Finished"
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
    @Published var chats: [String] = []
    @Published var tools: [ToolRecord] = []
    @Published var isActivityVisible = true
    @Published var isConnected = false

    func receive(_ event: DamonEvent) { conversation.apply(event) }
    func newChat() { conversation = ConversationState(); input = "" }
}

struct ToolRecord: Identifiable, Codable, Equatable, Sendable {
    let id: String
    let name: String
    let description: String
    let path: String
    let category: String
}
