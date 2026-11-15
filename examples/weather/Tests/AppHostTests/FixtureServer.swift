import Foundation
#if canImport(Glibc)
    import Glibc
#endif

/// A one-thread HTTP server on 127.0.0.1 that answers every request with
/// 503 while `failing` is set and with `body` otherwise: a forecast service
/// that is down, then back.
final class FixtureServer: @unchecked Sendable {
    let port: UInt16
    private let fd: Int32
    private let body: Data
    private let lock = NSLock()
    private var _failing = true

    var failing: Bool {
        get { lock.withLock { _failing } }
        set { lock.withLock { _failing = newValue } }
    }

    init(body: Data) throws {
        self.body = body
        #if canImport(Glibc)
            let socketFD = socket(AF_INET, Int32(SOCK_STREAM.rawValue), 0)
        #else
            let socketFD = socket(AF_INET, SOCK_STREAM, 0)
        #endif
        guard socketFD >= 0 else { throw POSIXError(.EIO) }
        var addr = sockaddr_in()
        #if !canImport(Glibc)
            addr.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        #endif
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = 0
        addr.sin_addr.s_addr = in_addr_t(0x7F00_0001).bigEndian
        var length = socklen_t(MemoryLayout<sockaddr_in>.size)
        let bound = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(socketFD, $0, length) }
        }
        let named = withUnsafeMutablePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(socketFD, $0, &length) }
        }
        guard bound == 0, named == 0, listen(socketFD, 16) == 0 else {
            close(socketFD)
            throw POSIXError(.EADDRINUSE)
        }
        fd = socketFD
        port = UInt16(bigEndian: addr.sin_port)
        let thread = Thread { [self] in serve() }
        thread.start()
    }

    deinit {
        shutdown(fd, Int32(SHUT_RDWR))
        close(fd)
    }

    private func serve() {
        while true {
            let client = accept(fd, nil, nil)
            guard client >= 0 else { return }
            var request = Data()
            var buffer = [UInt8](repeating: 0, count: 4096)
            while !request.contains(Data("\r\n\r\n".utf8)) {
                let n = read(client, &buffer, buffer.count)
                if n <= 0 { break }
                request.append(contentsOf: buffer[0..<n])
            }
            let (status, payload) = failing ? ("503 Service Unavailable", Data()) : ("200 OK", body)
            var response = Data(
                "HTTP/1.1 \(status)\r\nContent-Type: application/json\r\nContent-Length: \(payload.count)\r\nConnection: close\r\n\r\n"
                    .utf8)
            response.append(payload)
            _ = response.withUnsafeBytes { write(client, $0.baseAddress, $0.count) }
            close(client)
        }
    }
}
