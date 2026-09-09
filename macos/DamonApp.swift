import SwiftUI

private struct ChatResponse: Decodable {
    let response: String
}

private struct Message: Identifiable {
    let id = UUID()
    let fromDamon: Bool
    let text: String
}

@MainActor
private final class DamonSession: ObservableObject {
    @Published var messages = [
        Message(fromDamon: true, text: "Damon is ready. What would you like to do?")
    ]
    @Published var input = ""
    @Published var available = false

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
            try pipe.fileHandleForWriting.write(contentsOf: Data((text + "\n").utf8))
        } catch {
            available = false
            messages.append(Message(fromDamon: true, text: "I could not receive that request: \(error.localizedDescription)"))
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
        process.standardInput = inputPipe
        process.standardOutput = outputPipe
        process.standardError = errorPipe
        process.terminationHandler = { [weak self] task in
            Task { @MainActor in
                guard let self else { return }
                self.available = false
                if task.terminationStatus != 0 {
                    self.messages.append(Message(fromDamon: true, text: "The local runtime stopped unexpectedly."))
                }
            }
        }
        outputPipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            guard !data.isEmpty else { return }
            Task { @MainActor in self?.consume(data) }
        }
        errorPipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            guard !data.isEmpty, let text = String(data: data, encoding: .utf8) else { return }
            Task { @MainActor in
                self?.messages.append(Message(fromDamon: true, text: text.trimmingCharacters(in: .whitespacesAndNewlines)))
            }
        }
        do {
            try process.run()
            self.process = process
            self.inputPipe = inputPipe
            available = true
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
                let decoded = try JSONDecoder().decode(ChatResponse.self, from: Data(line))
                messages.append(Message(fromDamon: true, text: decoded.response))
            } catch {
                messages.append(Message(fromDamon: true, text: "The runtime returned an unreadable response."))
            }
        }
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
                .foregroundStyle(message.fromDamon ? .primary : .white)
                .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            if message.fromDamon { Spacer(minLength: 72) }
        }
    }
}

private struct ContentView: View {
    @StateObject private var session = DamonSession()

    var body: some View {
        VStack(spacing: 0) {
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
