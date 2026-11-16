import Foundation

/// Live numbers from one `stats` message. rx is download, tx is upload.
struct Stats {
    var rxMbps: Double
    var txMbps: Double
    var rxBytes: UInt64
    var txBytes: UInt64
}

private struct IPCMessage: Decodable {
    var type: String
    var state: String?
    var ip: String?
    var iface: String?
    var tx_mbps: Double?
    var rx_mbps: Double?
    var tx_bytes: UInt64?
    var rx_bytes: UInt64?
}

/// Talks to the daemon over its Unix socket: newline-delimited JSON, non-blocking reads.
final class IPCClient {
    private var sockFd: Int32 = -1
    private let sockPath = "/var/run/macostether.sock"
    private var readBuffer = ""
    private var eofCount = 0

    var onStats: ((Stats) -> Void)?
    var onState: ((String, String?, String?) -> Void)?
    /// Called when an open connection breaks.
    var onLost: (() -> Void)?

    var isConnected: Bool { sockFd >= 0 }

    func connect() -> Bool {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { return false }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let pathCapacity = MemoryLayout.size(ofValue: addr.sun_path)
        withUnsafeMutablePointer(to: &addr.sun_path) { ptr in
            sockPath.withCString { cstr in
                _ = memcpy(ptr, cstr, min(strlen(cstr) + 1, pathCapacity))
            }
        }

        let result = withUnsafePointer(to: &addr) { ptr in
            ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sockPtr in
                Darwin.connect(fd, sockPtr, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        if result < 0 {
            close(fd)
            return false
        }

        // Non-blocking only after a successful connect.
        let flags = fcntl(fd, F_GETFL, 0)
        if flags >= 0 {
            _ = fcntl(fd, F_SETFL, flags | O_NONBLOCK)
        }

        sockFd = fd
        readBuffer = ""
        eofCount = 0
        return true
    }

    /// Reads whatever the daemon has sent and dispatches complete lines.
    func poll() {
        guard sockFd >= 0 else { return }

        var buf = [UInt8](repeating: 0, count: 4096)
        while sockFd >= 0 {
            let n = Darwin.read(sockFd, &buf, buf.count)
            if n > 0 {
                eofCount = 0
                readBuffer += String(decoding: buf[0..<n], as: UTF8.self)
                processLines()
            } else if n == 0 {
                eofCount += 1
                if eofCount >= 3 { lose() }
                return
            } else {
                let err = errno
                if err != EAGAIN && err != EWOULDBLOCK { lose() }
                return
            }
        }
    }

    func send(_ command: String) {
        guard sockFd >= 0 else { return }
        let msg = "{\"type\":\"\(command)\"}\n"
        msg.withCString { cstr in
            _ = Darwin.write(sockFd, cstr, Int(strlen(cstr)))
        }
    }

    func disconnect() {
        if sockFd >= 0 {
            close(sockFd)
            sockFd = -1
        }
        readBuffer = ""
    }

    private func lose() {
        disconnect()
        onLost?()
    }

    private func processLines() {
        while let newline = readBuffer.firstIndex(of: "\n") {
            let line = String(readBuffer[..<newline])
            readBuffer = String(readBuffer[readBuffer.index(after: newline)...])

            guard !line.isEmpty,
                  let msg = try? JSONDecoder().decode(IPCMessage.self, from: Data(line.utf8)) else {
                continue
            }

            if msg.type == "stats", let rx = msg.rx_mbps, let tx = msg.tx_mbps {
                onStats?(Stats(rxMbps: rx, txMbps: tx,
                               rxBytes: msg.rx_bytes ?? 0, txBytes: msg.tx_bytes ?? 0))
            } else if msg.type == "state", let state = msg.state {
                onState?(state, msg.ip, msg.iface)
            }
        }
    }
}
