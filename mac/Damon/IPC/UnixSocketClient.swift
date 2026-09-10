import Darwin
import Foundation

enum IPCError: Error, Equatable { case pathTooLong, connectFailed(Int32), disconnected }

actor UnixSocketClient {
    private var descriptor: Int32 = -1

    func connect(path: String) throws {
        disconnect()
        descriptor = socket(AF_UNIX, SOCK_STREAM, 0)
        guard descriptor >= 0 else { throw IPCError.connectFailed(errno) }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let capacity = MemoryLayout.size(ofValue: address.sun_path)
        guard path.utf8.count < capacity else { disconnect(); throw IPCError.pathTooLong }
        withUnsafeMutableBytes(of: &address.sun_path) { buffer in
            buffer.initializeMemory(as: UInt8.self, repeating: 0)
            path.utf8CString.withUnsafeBytes { source in buffer.copyBytes(from: source) }
        }
        let result = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(descriptor, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard result == 0 else { let code = errno; disconnect(); throw IPCError.connectFailed(code) }
    }

    func send(_ object: [String: String]) throws {
        guard descriptor >= 0 else { throw IPCError.disconnected }
        var data = try JSONSerialization.data(withJSONObject: object)
        data.append(0x0A)
        try data.withUnsafeBytes { bytes in
            var sent = 0
            while sent < bytes.count {
                let count = Darwin.write(descriptor, bytes.baseAddress!.advanced(by: sent), bytes.count - sent)
                guard count > 0 else { throw IPCError.disconnected }
                sent += count
            }
        }
    }

    func disconnect() {
        if descriptor >= 0 { Darwin.close(descriptor); descriptor = -1 }
    }
}
