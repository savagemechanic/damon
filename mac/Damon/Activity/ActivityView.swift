import SwiftUI

struct ActivityView: View {
    let state: ConversationState
    var promote: (() -> Void)? = nil
    var cancel: (() -> Void)? = nil
    var body: some View {
        Form {
            LabeledContent("Status", value: state.status)
            if let duration = state.duration { LabeledContent("Duration", value: String(format: "%.2fs", duration)) }
            if let code = state.exitCode { LabeledContent("Exit status", value: String(code)) }
            if !state.changedFiles.isEmpty {
                Section("Changed files") { ForEach(state.changedFiles, id: \.self) { Text($0).font(.caption).textSelection(.enabled) } }
            }
            if state.status == "Executing Python" { Button("Cancel Execution", role: .destructive) { cancel?() } }
            if state.scriptPath != nil { Button("Save Python as Tool") { promote?() } }
            if let error = state.error { Text(error).foregroundStyle(.red) }
        }.formStyle(.grouped).frame(minWidth: 210)
    }
}
