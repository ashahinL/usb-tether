import Foundation
import ServiceManagement

enum TetherStatus {
    case connected
    case waiting
    case off
    case serviceDown

    var title: String {
        switch self {
        case .connected: return "Connected"
        case .waiting: return "Waiting for phone"
        case .off: return "Off"
        case .serviceDown: return "Service not running"
        }
    }
}

struct SpeedSample {
    var rxMbps: Double
    var txMbps: Double
}

/// App state for the popover and the menu-bar icon. Lives on the main thread.
final class TetherModel: ObservableObject {
    static let historyLength = 60

    @Published var status: TetherStatus = .serviceDown
    @Published var ip: String?
    @Published var iface: String?
    @Published var stats: Stats?
    @Published var history: [SpeedSample] = []
    @Published var connectedSince: Date?
    @Published var now = Date()
    @Published var openAtLogin = false
    @Published var loginItemNote: String?

    @Published var autoConnect: Bool {
        didSet {
            defaults.set(autoConnect, forKey: Keys.autoConnect)
            ipc.send(autoConnect ? "enable" : "disable")
            if daemonState == nil || daemonState == "disconnected" { updateStatus() }
        }
    }

    @Published var showSpeedInMenuBar: Bool {
        didSet { defaults.set(showSpeedInMenuBar, forKey: Keys.showSpeed) }
    }

    private enum Keys {
        static let autoConnect = "autoConnect"
        static let showSpeed = "showSpeedInMenuBar"
    }

    private let defaults = UserDefaults.standard
    private let ipc = IPCClient()
    private var daemonState: String?
    private var timer: Timer?

    init() {
        defaults.register(defaults: [Keys.autoConnect: true, Keys.showSpeed: true])
        autoConnect = defaults.bool(forKey: Keys.autoConnect)
        showSpeedInMenuBar = defaults.bool(forKey: Keys.showSpeed)
    }

    /// Connects to the daemon and starts the once-a-second update loop.
    func start() {
        ipc.onState = { [weak self] state, ip, iface in self?.apply(state: state, ip: ip, iface: iface) }
        ipc.onStats = { [weak self] stats in self?.apply(stats: stats) }
        ipc.onLost = { [weak self] in self?.serviceLost() }
        refreshLoginItem()
        tick()
        timer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
            self?.tick()
        }
    }

    func stop() {
        timer?.invalidate()
        ipc.disconnect()
    }

    // MARK: - Actions

    func connect() {
        ipc.send("enable")
    }

    /// With auto-connect on, the daemon keeps watching after the session ends.
    func disconnect() {
        ipc.send(autoConnect ? "stop" : "disable")
    }

    func setOpenAtLogin(_ enabled: Bool) {
        do {
            if enabled {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
            loginItemNote = nil
        } catch {
            loginItemNote = "Could not change this: \(error.localizedDescription)"
        }
        refreshLoginItem()
    }

    func refreshLoginItem() {
        let status = SMAppService.mainApp.status
        openAtLogin = status == .enabled
        if status == .requiresApproval {
            loginItemNote = "Allow USB Tether in System Settings > General > Login Items."
        }
    }

    func openLoginItemsSettings() {
        SMAppService.openSystemSettingsLoginItems()
    }

    // MARK: - Updates

    private func tick() {
        now = Date()
        if ipc.isConnected {
            ipc.poll()
            return
        }
        if ipc.connect() {
            if autoConnect { ipc.send("enable") }
            updateStatus()
            ipc.poll()
        } else if status != .serviceDown {
            serviceLost()
        }
    }

    private func apply(state: String, ip: String?, iface: String?) {
        daemonState = state
        if state == "connected" {
            self.ip = ip
            self.iface = iface
        }
        updateStatus()
    }

    private func apply(stats: Stats) {
        guard status == .connected else { return }
        self.stats = stats
        history.append(SpeedSample(rxMbps: stats.rxMbps, txMbps: stats.txMbps))
        if history.count > Self.historyLength {
            history.removeFirst(history.count - Self.historyLength)
        }
    }

    private func serviceLost() {
        daemonState = nil
        updateStatus()
    }

    private func updateStatus() {
        let newStatus: TetherStatus
        if !ipc.isConnected {
            newStatus = .serviceDown
        } else {
            switch daemonState {
            case "connected": newStatus = .connected
            case "watching": newStatus = .waiting
            case "idle": newStatus = .off
            default: newStatus = autoConnect ? .waiting : .off
            }
        }

        if newStatus == .connected {
            if connectedSince == nil { connectedSince = Date() }
        } else {
            connectedSince = nil
            stats = nil
            history = []
            ip = nil
            iface = nil
        }
        if newStatus != status { status = newStatus }
    }
}
