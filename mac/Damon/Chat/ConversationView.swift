import SwiftUI

struct ConversationView: View {
    @ObservedObject var model: AppModel
    @AppStorage("reasoningVisibility") private var reasoningVisibility = true

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 14) {
                    if model.messages.isEmpty && model.conversation.answer.isEmpty && model.conversation.python.isEmpty {
                        ContentUnavailableView("Ask Damon", systemImage: "terminal", description: Text("Generated Python and real output appear here."))
                    }
                    ForEach(Array(model.messages.enumerated()), id: \.offset) { _, message in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(message.role == "user" ? "You" : "Damon").font(.caption).foregroundStyle(.secondary)
                            Text(message.content).textSelection(.enabled)
                        }.frame(maxWidth: .infinity, alignment: .leading)
                    }
                    if reasoningVisibility {
                        DisclosureGroup("Thinking") { Text(model.conversation.reasoning).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) }
                            .disabled(model.conversation.reasoning.isEmpty)
                    }
                    DisclosureGroup("Python") { Text(model.conversation.python).font(.system(.body, design: .monospaced)).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) }
                        .disabled(model.conversation.python.isEmpty)
                    DisclosureGroup("stdout / stderr") {
                        Text(model.conversation.stdout).font(.system(.body, design: .monospaced)).foregroundStyle(.primary)
                        Text(model.conversation.stderr).font(.system(.body, design: .monospaced)).foregroundStyle(.red)
                    }.disabled(model.conversation.stdout.isEmpty && model.conversation.stderr.isEmpty)
                    if model.conversation.status != "Finished" { Text(model.conversation.answer).textSelection(.enabled) }
                }.padding()
            }
            Divider()
            HStack {
                TextField("Ask Damon…", text: $model.input, axis: .vertical)
                    .lineLimit(1...5)
                    .onSubmit { model.send() }
                Button("Send") { model.send() }
                    .keyboardShortcut(.return, modifiers: .command)
                    .disabled(model.input.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !model.isConnected || model.selectedModel.isEmpty || model.conversation.isRunActive)
            }.padding()
        }
    }
}
