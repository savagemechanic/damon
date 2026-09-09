import SwiftUI

private struct ChatEvent: Decodable {
    let type: String?
    let state: String?
    let response: String?
    let models: [String]?
    let selected: String?
    let model: String?
    let message: String?
    let ok: Bool?
}

private struct Message: Identifiable {
    let id = UUID()
    let fromDamon: Bool
    let text: String
}

@MainActor
private final class DamonSession: ObservableObject, @unchecked Sendable {
    @Published var messages = [
        Message(fromDamon: true, text: "Damon is ready. What would you like to do?")
    ]
    @Published var input = ""
    @Published var available = false
    @Published var models: [String] = []
    @Published var selectedModel = ""
    @Published var state = "Starting"
    @Published var modelMessage = ""

    private var process: Process?
    private var inputPipe: Pipe?
    private var pending = Data()

    init() {
        start()
    }

    deinit {
        process?.terminate()
    }

    func send() {
        let text = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard available, !text.isEmpty, let pipe = inputPipe else { return }
        input = ""
        messages.append(Message(fromDamon: false, text: text))
        do {
            try writeCommand(["type": "request", "text": text], to: pipe)
        } catch {
            available = false
            messages.append(Message(fromDamon: true, text: "I could not receive that request: \(error.localizedDescription)"))
        }
    }

    func refreshModels() {
        guard let pipe = inputPipe else { return }
        do {
            modelMessage = ""
            try writeCommand(["type": "list_models"], to: pipe)
        } catch {
            modelMessage = "Could not ask Ollama for its models."
        }
    }

    func selectModel(_ model: String) {
        guard !model.isEmpty, let pipe = inputPipe else { return }
        do {
            try writeCommand(["type": "select_model", "model": model], to: pipe)
        } catch {
            modelMessage = "Could not select \(model)."
        }
    }

    private func start() {
        guard let executable = Bundle.main.url(forResource: "damon", withExtension: nil) else {
            messages.append(Message(fromDamon: true, text: "The Damon runtime is missing from this application."))
            return
        }
        let process = Process()
        let inputPipe = Pipe()
        let outputPipe = Pipe()
        let errorPipe = Pipe()
        process.executableURL = executable
        process.arguments = ["--chat-stdio"]
        process.currentDirectoryURL = FileManager.default.homeDirectoryForCurrentUser
        var environment = ProcessInfo.processInfo.environment
        environment["PATH"] = [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            environment["PATH"] ?? "",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin"
        ].joined(separator: ":")
        process.environment = environment
        process.standardInput = inputPipe
        process.standardOutput = outputPipe
        process.standardError = errorPipe
        process.terminationHandler = { [self] task in
            Task { @MainActor [self] in
                self.available = false
                if task.terminationStatus != 0 {
                    self.messages.append(Message(fromDamon: true, text: "The local runtime stopped unexpectedly."))
                }
            }
        }
        outputPipe.fileHandleForReading.readabilityHandler = { [self] handle in
            let data = handle.availableData
            guard !data.isEmpty else { return }
            Task { @MainActor [self] in consume(data) }
        }
        errorPipe.fileHandleForReading.readabilityHandler = { [self] handle in
            let data = handle.availableData
            guard !data.isEmpty, let text = String(data: data, encoding: .utf8) else { return }
            Task { @MainActor [self] in
                messages.append(Message(fromDamon: true, text: text.trimmingCharacters(in: .whitespacesAndNewlines)))
            }
        }
        do {
            try process.run()
            self.process = process
            self.inputPipe = inputPipe
            available = true
            state = "Ready"
            refreshModels()
        } catch {
            messages.append(Message(fromDamon: true, text: "Damon could not start: \(error.localizedDescription)"))
        }
    }

    private func consume(_ data: Data) {
        pending.append(data)
        while let newline = pending.firstIndex(of: 0x0a) {
            let line = pending[..<newline]
            pending.removeSubrange(...newline)
            guard !line.isEmpty else { continue }
            do {
                let event = try JSONDecoder().decode(ChatEvent.self, from: Data(line))
                consume(event)
            } catch {
                messages.append(Message(fromDamon: true, text: "The runtime returned an unreadable response."))
            }
        }
    }

    private func consume(_ event: ChatEvent) {
        switch event.type ?? "response" {
        case "state":
            state = displayState(event.state ?? "processing")
        case "response":
            if let response = event.response {
                messages.append(Message(fromDamon: true, text: response))
            }
        case "models":
            models = event.models ?? []
            let configured = event.selected ?? ""
            let preferred = UserDefaults.standard.string(forKey: "ollamaModel") ?? ""
            let choice = models.contains(preferred)
                ? preferred
                : (models.contains(configured) ? configured : (models.first ?? ""))
            selectedModel = choice
            if !choice.isEmpty && choice != configured {
                selectModel(choice)
            }
            modelMessage = models.isEmpty ? "No Ollama models installed" : "Ollama connected"
        case "model_selected":
            if event.ok == true {
                selectedModel = event.model ?? selectedModel
                UserDefaults.standard.set(selectedModel, forKey: "ollamaModel")
            }
            modelMessage = event.message ?? ""
        case "model_error":
            models = []
            selectedModel = ""
            modelMessage = "Ollama is not connected"
        default:
            break
        }
    }

    private func displayState(_ value: String) -> String {
        switch value {
        case "asking_ollama": return "Asking Ollama"
        case "thinking": return "Thinking"
        case "checking_meaning": return "Checking meaning"
        case "running": return "Running"
        case "verifying": return "Verifying"
        case "learning": return "Learning"
        case "ready": return "Ready"
        default: return "Processing"
        }
    }

    private func writeCommand(_ command: [String: String], to pipe: Pipe) throws {
        var data = try JSONSerialization.data(withJSONObject: command)
        data.append(0x0a)
        try pipe.fileHandleForWriting.write(contentsOf: data)
    }
}

private struct MessageBubble: View {
    let message: Message

    var body: some View {
        HStack {
            if message.fromDamon == false { Spacer(minLength: 72) }
            Text(message.text)
                .textSelection(.enabled)
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .background(message.fromDamon ? Color(nsColor: .controlBackgroundColor) : Color.accentColor)
                .foregroundColor(message.fromDamon ? Color.primary : Color.white)
                .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            if message.fromDamon { Spacer(minLength: 72) }
        }
    }
}

private struct ContentView: View {
    @StateObject private var session = DamonSession()

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Circle()
                    .fill(session.state == "Ready" ? Color.green : Color.orange)
                    .frame(width: 8, height: 8)
                Text(session.state)
                    .font(.callout)
                Spacer()
                if !session.models.isEmpty {
                    Picker("Model", selection: $session.selectedModel) {
                        ForEach(session.models, id: \.self) { model in
                            Text(model).tag(model)
                        }
                    }
                    .labelsHidden()
                    .frame(maxWidth: 260)
                    .onChange(of: session.selectedModel) { model in
                        session.selectModel(model)
                    }
                } else {
                    Text(session.modelMessage)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
                Button(action: session.refreshModels) {
                    Image(systemName: "arrow.clockwise")
                }
                .help("Refresh Ollama models")
                .disabled(!session.available)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            Divider()
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 12) {
                        ForEach(session.messages) { message in
                            MessageBubble(message: message).id(message.id)
                        }
                    }
                    .padding(18)
                }
                .onChange(of: session.messages.count) { _ in
                    if let id = session.messages.last?.id {
                        withAnimation { proxy.scrollTo(id, anchor: .bottom) }
                    }
                }
            }
            Divider()
            HStack(alignment: .bottom, spacing: 10) {
                TextField("Message Damon", text: $session.input, axis: .vertical)
                    .textFieldStyle(.roundedBorder)
                    .lineLimit(1...6)
                    .onSubmit(session.send)
                Button("Send", action: session.send)
                    .keyboardShortcut(.return, modifiers: [.command])
                    .disabled(!session.available || session.input.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
            .padding(14)
        }
        .frame(minWidth: 620, minHeight: 520)
    }
}

@main
private struct DamonApp: App {
    var body: some Scene {
        WindowGroup("Damon") {
            ContentView()
        }
        .defaultSize(width: 760, height: 680)
    }
}
