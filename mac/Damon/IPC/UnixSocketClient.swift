import Darwin
import Foundation

enum IPCError: Error, Equatable { case pathTooLong, connectFailed(Int32), disconnected, invalidResponse }

final class UnixSocketClient: @unchecked Sendable {
    let path: String
    init(path: String) { self.path = path }

    func request<T: Encodable & Sendable>(_ value: T) async throws -> [Data] {
        var lines: [Data] = []
        for try await line in try stream(value) { lines.append(line) }
        guard !lines.isEmpty else { throw IPCError.invalidResponse }
        return lines
    }

    func stream<T: Encodable & Sendable>(_ value: T) throws -> AsyncThrowingStream<Data, Error> {
        let request = try JSONEncoder().encode(value)
        return AsyncThrowingStream { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                do { try self.perform(request, continuation: continuation); continuation.finish() }
                catch { continuation.finish(throwing: error) }
            }
        }
    }

    private func perform(_ request: Data, continuation: AsyncThrowingStream<Data, Error>.Continuation) throws {
        let descriptor = socket(AF_UNIX, SOCK_STREAM, 0)
        guard descriptor >= 0 else { throw IPCError.connectFailed(errno) }
        defer { Darwin.close(descriptor) }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let capacity = MemoryLayout.size(ofValue: address.sun_path)
        guard path.utf8.count < capacity else { throw IPCError.pathTooLong }
        withUnsafeMutableBytes(of: &address.sun_path) { buffer in
            buffer.initializeMemory(as: UInt8.self, repeating: 0)
            path.utf8CString.withUnsafeBytes { buffer.copyBytes(from: $0) }
        }
        let connected = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(descriptor, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard connected == 0 else { throw IPCError.connectFailed(errno) }
        var bytes = request
        bytes.append(0x0A)
        try bytes.withUnsafeBytes { buffer in
            var sent = 0
            while sent < buffer.count {
                let count = Darwin.write(descriptor, buffer.baseAddress!.advanced(by: sent), buffer.count - sent)
                guard count > 0 else { throw IPCError.disconnected }
                sent += count
            }
        }
        shutdown(descriptor, SHUT_WR)
        var pending = Data()
        var buffer = [UInt8](repeating: 0, count: 8192)
        while true {
            let count = Darwin.read(descriptor, &buffer, buffer.count)
            if count == 0 { break }
            guard count > 0 else { throw IPCError.disconnected }
            pending.append(buffer, count: count)
            while let newline = pending.firstIndex(of: 0x0A) {
                let line = pending[..<newline]
                if !line.isEmpty { continuation.yield(Data(line)) }
                pending.removeSubrange(...newline)
            }
        }
        if !pending.isEmpty { continuation.yield(pending) }
    }
}

struct IPCRequest: Codable, Sendable {
    let type: String
    var apiKey: String? = nil
    var message: String? = nil
    var model: String? = nil
    var thinkingEffort: String? = nil
    var systemPrompt: String? = nil
    var workingDirectory: String? = nil
    var pythonExecutable: String? = nil
    var maxTurns: Int? = nil
    var executionTimeout: Double? = nil
    var path: String? = nil
    var name: String? = nil
    var description: String? = nil
    var category: String? = nil
    var chatId: String? = nil

    enum CodingKeys: String, CodingKey {
        case type, message, model, path, name, description, category
        case apiKey = "api_key"
        case thinkingEffort = "thinking_effort"
        case systemPrompt = "system_prompt"
        case workingDirectory = "working_directory"
        case pythonExecutable = "python_executable"
        case maxTurns = "max_turns"
        case executionTimeout = "execution_timeout"
        case chatId = "chat_id"
    }
}

struct ZenModel: Codable, Equatable, Sendable {
    let id: String
    let name: String
    let reasoningEfforts: [String]
    enum CodingKeys: String, CodingKey { case id, name; case reasoningEfforts = "reasoning_efforts" }
}

struct IPCResponse: Codable, Sendable {
    let type: String
    var message: String? = nil
    var source: String? = nil
    var models: [ZenModel]? = nil
    var tools: [ToolRecord]? = nil
    var chats: [ChatRecord]? = nil
    var messages: [MessageRecord]? = nil
}
