import SwiftUI

struct ConversationView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 14) {
                    if model.conversation.answer.isEmpty && model.conversation.python.isEmpty {
                        ContentUnavailableView("Ask Damon", systemImage: "terminal", description: Text("Generated Python and real output appear here."))
                    }
                    DisclosureGroup("Thinking") { Text(model.conversation.reasoning).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) }
                        .disabled(model.conversation.reasoning.isEmpty)
                    DisclosureGroup("Python") { Text(model.conversation.python).font(.system(.body, design: .monospaced)).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) }
                        .disabled(model.conversation.python.isEmpty)
                    DisclosureGroup("stdout / stderr") {
                        Text(model.conversation.stdout).font(.system(.body, design: .monospaced)).foregroundStyle(.primary)
                        Text(model.conversation.stderr).font(.system(.body, design: .monospaced)).foregroundStyle(.red)
                    }.disabled(model.conversation.stdout.isEmpty && model.conversation.stderr.isEmpty)
                    Text(model.conversation.answer).textSelection(.enabled)
                }.padding()
            }
            Divider()
            HStack {
                TextField("Ask Damon…", text: $model.input, axis: .vertical)
                    .lineLimit(1...5)
                    .onSubmit { model.send() }
                Button("Send") { model.send() }
                    .keyboardShortcut(.return, modifiers: .command)
                    .disabled(model.input.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !model.isConnected || model.selectedModel.isEmpty)
            }.padding()
        }
    }
}
