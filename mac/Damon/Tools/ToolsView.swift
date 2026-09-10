import SwiftUI

struct ToolsView: View {
    let tools: [ToolRecord]
    var body: some View {
        List(tools) { tool in
            VStack(alignment: .leading) { Text(tool.name); Text(tool.description).font(.caption).foregroundStyle(.secondary); Text(tool.path).font(.caption2).textSelection(.enabled) }
        }.overlay { if tools.isEmpty { ContentUnavailableView("No reusable tools", systemImage: "hammer", description: Text("Promoted Python scripts appear here.")) } }
    }
}
