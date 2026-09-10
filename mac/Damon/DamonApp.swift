import SwiftUI

@main
struct DamonApp: App {
    @StateObject private var model = AppModel()
    var body: some Scene {
        WindowGroup("Damon") {
            HSplitView {
                List(selection: $model.sidebarSelection) {
                    Section("Chats") { ForEach(model.chats) { Text($0.title) } }
                    Section("Library") { Label("Tools", systemImage: "hammer").tag("tools") }
                }
                .frame(minWidth: 170, idealWidth: 190, maxWidth: 230)
                VStack(spacing: 0) {
                    HStack { Text("Conversation").font(.headline); Spacer() }.padding(12)
                    Divider()
                    if model.sidebarSelection == "tools" { ToolsView(tools: model.tools) } else { ConversationView(model: model) }
                }.frame(minWidth: 500, maxWidth: .infinity)
                if model.isActivityVisible {
                    VStack(spacing: 0) {
                        HStack { Text("Activity").font(.headline); Spacer() }.padding(12)
                        Divider()
                        ActivityView(state: model.conversation, promote: model.promoteCurrentScript)
                    }.frame(minWidth: 220, idealWidth: 250, maxWidth: 300)
                }
            }.frame(minWidth: 960, minHeight: 640)
                .task { model.start() }
                .onReceive(NotificationCenter.default.publisher(for: NSApplication.willTerminateNotification)) { _ in model.stop() }
        }
        .commands {
            CommandGroup(after: .newItem) { Button("New Chat") { model.newChat(); model.sidebarSelection = "conversation" }.keyboardShortcut("n"); Button("Tools Library") { model.sidebarSelection = "tools" }; Button("Open Tools Folder") { NSWorkspace.shared.open(FileManager.default.homeDirectoryForCurrentUser.appending(path: ".damon/tools")) } }
        }
        Settings { SettingsView(model: model) }
    }
}
