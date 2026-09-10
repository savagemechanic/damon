import SwiftUI

struct ActivityView: View {
    let state: ConversationState
    var body: some View {
        Form {
            LabeledContent("Status", value: state.status)
            if let duration = state.duration { LabeledContent("Duration", value: String(format: "%.2fs", duration)) }
            if let code = state.exitCode { LabeledContent("Exit status", value: String(code)) }
            if let error = state.error { Text(error).foregroundStyle(.red) }
        }.formStyle(.grouped).frame(minWidth: 210)
    }
}
