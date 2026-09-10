import SwiftUI

struct SettingsView: View {
    @ObservedObject var model: AppModel
    @StateObject private var settings = SettingsModel()
    private var efforts: [String] { model.models.first(where: { $0.id == model.selectedModel })?.reasoningEfforts ?? [] }
    var body: some View {
        TabView {
            Form {
                SecureField("API key", text: $settings.apiKey).accessibilityIdentifier("zen-api-key")
                HStack { Button("Save") { try? settings.saveKey(); model.configure(apiKey: settings.apiKey) }; Button("Delete", role: .destructive) { try? settings.deleteKey(); model.clearAPIKey() }; Button("Test Connection") { settings.apiKey.isEmpty ? model.reloadConfiguration() : model.configure(apiKey: settings.apiKey) } }
                Picker("Model", selection: $model.selectedModel) { ForEach(model.models, id: \.id) { Text($0.name).tag($0.id) } }
                    .onChange(of: model.selectedModel) { _, value in UserDefaults.standard.set(value, forKey: "selectedModel") }
                Picker("Thinking effort", selection: $settings.thinkingEffort) { ForEach(efforts, id: \.self) { Text($0) } }.disabled(efforts.isEmpty)
                Text(model.conversation.status).foregroundStyle(.secondary)
            }.padding().tabItem { Label("Model", systemImage: "brain") }
            Form { TextEditor(text: $settings.systemPrompt).font(.system(.body, design: .monospaced)) }.padding().tabItem { Label("Prompt", systemImage: "text.quote") }
            Form { TextField("Python executable", text: $settings.pythonExecutable); Stepper("Max turns: \(settings.maxTurns)", value: $settings.maxTurns, in: 1...32); Stepper("Timeout: \(Int(settings.executionTimeout))s", value: $settings.executionTimeout, in: 1...600) }.padding().tabItem { Label("Runtime", systemImage: "terminal") }
            Form { Toggle("Provider reasoning visibility", isOn: $settings.reasoningVisibility); Toggle("Raw event logging", isOn: $settings.rawEventLogging) }.padding().tabItem { Label("Advanced", systemImage: "gearshape.2") }
        }.frame(width: 560, height: 380)
    }
}
