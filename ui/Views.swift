import AppKit
import SwiftUI

enum Palette {
    static let download = Color(nsColor: .systemBlue)
    static let upload = Color(nsColor: .systemOrange)

    static func dot(_ status: TetherStatus) -> Color {
        switch status {
        case .connected: return Color(nsColor: .systemGreen)
        case .waiting: return Color(nsColor: .systemOrange)
        case .off: return Color(nsColor: .systemGray)
        case .serviceDown: return Color(nsColor: .systemRed)
        }
    }
}

enum Format {
    static func speed(_ mbps: Double) -> String {
        String(format: "%.1f", mbps)
    }

    static func bytes(_ count: UInt64) -> String {
        let formatter = ByteCountFormatter()
        formatter.allowedUnits = [.useMB, .useGB]
        formatter.countStyle = .file
        formatter.allowsNonnumericFormatting = false
        return formatter.string(fromByteCount: Int64(clamping: count))
    }

    static func duration(from start: Date, to end: Date) -> String {
        let total = max(0, Int(end.timeIntervalSince(start)))
        let hours = total / 3600
        let minutes = (total % 3600) / 60
        let seconds = total % 60
        if hours > 0 {
            return String(format: "%d:%02d:%02d", hours, minutes, seconds)
        }
        return String(format: "%d:%02d", minutes, seconds)
    }
}

/// The popover content. It draws no background of its own, so the popover's
/// translucent material shows through, as in the system menu-bar menus.
struct PopoverView: View {
    @ObservedObject var model: TetherModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HeaderView(model: model)
                .padding(.leading, 16)
                .padding(.trailing, 10)
                .padding(.vertical, 12)
            SectionDivider()
            StatusSection(model: model)
                .padding(.horizontal, 16)
                .padding(.vertical, 14)
            SectionDivider()
            ControlsSection(model: model)
                .padding(.horizontal, 16)
                .padding(.top, 12)
                .padding(.bottom, 14)
        }
        .frame(width: 320)
    }
}

/// An inset separator, like the ones between groups in a menu.
struct SectionDivider: View {
    var body: some View {
        Divider().padding(.horizontal, 12)
    }
}

struct HeaderView: View {
    @ObservedObject var model: TetherModel

    var body: some View {
        HStack(spacing: 12) {
            Image(nsImage: NSApp?.applicationIconImage ?? NSImage())
                .resizable()
                .frame(width: 36, height: 36)
            VStack(alignment: .leading, spacing: 3) {
                Text("USB Tether")
                    .font(.headline)
                HStack(spacing: 6) {
                    Circle()
                        .fill(Palette.dot(model.status))
                        .frame(width: 8, height: 8)
                    Text(model.status.title)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: 8)
            SettingsMenu(model: model)
        }
    }
}

/// The gear menu: the settings people change once, plus About and Quit.
struct SettingsMenu: View {
    @ObservedObject var model: TetherModel

    var body: some View {
        Menu {
            Toggle("Open at Login", isOn: Binding(
                get: { model.openAtLogin },
                set: { model.setOpenAtLogin($0) }))
            Toggle("Show Speed in Menu Bar", isOn: $model.showSpeedInMenuBar)
            Divider()
            Button("About USB Tether") { AboutPanel.show() }
            Divider()
            Button("Quit USB Tether") { NSApp.terminate(nil) }
                .keyboardShortcut("q")
        } label: {
            Image(systemName: "gearshape")
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .help("Settings")
        .accessibilityLabel("Settings")
    }
}

struct StatusSection: View {
    @ObservedObject var model: TetherModel

    var body: some View {
        switch model.status {
        case .connected:
            ConnectedView(model: model)
        case .waiting:
            MessageView(symbol: "cable.connector", tint: .secondary,
                        text: "Plug in your phone and turn on USB tethering in Android settings.")
        case .off:
            MessageView(symbol: "power", tint: .secondary,
                        text: "Auto-connect is off. Click Connect to use your phone's internet.")
        case .serviceDown:
            MessageView(symbol: "exclamationmark.triangle.fill", tint: Palette.dot(.serviceDown),
                        text: "The background service is not running. Reinstall USB Tether to fix this.")
        }
    }
}

struct MessageView: View {
    let symbol: String
    let tint: Color
    let text: String

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: symbol)
                .font(.system(size: 15))
                .foregroundStyle(tint)
                .frame(width: 20)
            Text(text)
                .font(.callout)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
        }
    }
}

struct ConnectedView: View {
    @ObservedObject var model: TetherModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                SpeedTile(title: "Download", symbol: "arrow.down", tint: Palette.download,
                          mbps: model.stats?.rxMbps ?? 0, history: model.history.map(\.rxMbps))
                SpeedTile(title: "Upload", symbol: "arrow.up", tint: Palette.upload,
                          mbps: model.stats?.txMbps ?? 0, history: model.history.map(\.txMbps))
            }

            VStack(spacing: 6) {
                InfoRow(label: "Session data") {
                    HStack(spacing: 10) {
                        Label(Format.bytes(model.stats?.rxBytes ?? 0), systemImage: "arrow.down")
                            .foregroundStyle(Palette.download)
                        Label(Format.bytes(model.stats?.txBytes ?? 0), systemImage: "arrow.up")
                            .foregroundStyle(Palette.upload)
                    }
                    .labelStyle(CompactLabelStyle())
                }
                if let since = model.connectedSince {
                    InfoRow(label: "Connected for") {
                        Text(Format.duration(from: since, to: model.now))
                    }
                }
                if let ip = model.ip {
                    InfoRow(label: "IP address") {
                        Text(ip).textSelection(.enabled)
                    }
                    .help(addressHelp(ip))
                }
            }
        }
    }

    private func addressHelp(_ ip: String) -> String {
        let base = "This Mac's address on the phone's network"
        guard let iface = model.iface, !iface.isEmpty else { return base + "." }
        return base + " (interface \(iface))."
    }
}

struct CompactLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 2) {
            configuration.icon.font(.caption2.weight(.bold))
            configuration.title.foregroundStyle(Color.primary)
        }
    }
}

struct InfoRow<Value: View>: View {
    let label: String
    @ViewBuilder let value: Value

    var body: some View {
        HStack {
            Text(label)
                .foregroundStyle(.secondary)
            Spacer()
            value.monospacedDigit()
        }
        .font(.callout)
    }
}

/// Current speed in one direction, with its own graph of the last minute.
struct SpeedTile: View {
    let title: String
    let symbol: String
    let tint: Color
    let mbps: Double
    let history: [Double]

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Label(title, systemImage: symbol)
                .font(.caption)
                .foregroundStyle(.secondary)
                .labelStyle(TintedIconLabelStyle(tint: tint))
            HStack(alignment: .firstTextBaseline, spacing: 3) {
                Text(Format.speed(mbps))
                    .font(.system(size: 22, weight: .semibold, design: .rounded))
                    .monospacedDigit()
                Text("Mbit/s")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            SpeedGraph(values: history, capacity: TetherModel.historyLength, tint: tint)
                .frame(height: 40)
                .padding(.top, 4)
                .help("The last 60 seconds. Peak is the highest speed in that time.")
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color.primary.opacity(0.06)))
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(title) \(Format.speed(mbps)) megabits per second")
    }
}

struct TintedIconLabelStyle: LabelStyle {
    let tint: Color

    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 4) {
            configuration.icon.foregroundStyle(tint).fontWeight(.bold)
            configuration.title
        }
    }
}

/// One speed over the last `capacity` seconds, newest on the right.
/// The samples are joined with straight lines, with no smoothing. The top of the
/// plot is the highest sample in the window; a dashed line and label mark it.
struct SpeedGraph: View {
    let values: [Double]
    let capacity: Int
    let tint: Color

    private static let labelHeight: CGFloat = 12

    var body: some View {
        let peak = values.max() ?? 0
        VStack(alignment: .trailing, spacing: 0) {
            Text("Peak \(Format.speed(peak))")
                .font(.system(size: 9.5, weight: .medium))
                .monospacedDigit()
                .foregroundStyle(.secondary)
                .frame(height: Self.labelHeight, alignment: .top)
            GeometryReader { geo in
                let pts = points(peak: peak, size: geo.size)
                ZStack(alignment: .top) {
                    Path { path in
                        path.move(to: .zero)
                        path.addLine(to: CGPoint(x: geo.size.width, y: 0))
                    }
                    .stroke(Color.secondary.opacity(0.45), style: StrokeStyle(lineWidth: 0.5, dash: [2, 2]))
                    Path { path in
                        path.move(to: CGPoint(x: 0, y: geo.size.height))
                        path.addLine(to: CGPoint(x: geo.size.width, y: geo.size.height))
                    }
                    .stroke(Color.secondary.opacity(0.3), lineWidth: 0.5)
                    if pts.count > 1 {
                        area(pts, height: geo.size.height)
                            .fill(LinearGradient(colors: [tint.opacity(0.28), tint.opacity(0.04)],
                                                 startPoint: .top, endPoint: .bottom))
                        line(pts).stroke(tint, style: StrokeStyle(lineWidth: 1.25, lineJoin: .round))
                    }
                    if let last = pts.last {
                        Circle()
                            .fill(tint)
                            .frame(width: 4, height: 4)
                            .position(last)
                    }
                }
            }
        }
        .accessibilityHidden(true)
    }

    private func points(peak: Double, size: CGSize) -> [CGPoint] {
        let top = max(peak, 0.1)
        let step = size.width / CGFloat(max(1, capacity - 1))
        let offset = capacity - values.count
        return values.enumerated().map { index, value in
            CGPoint(x: CGFloat(offset + index) * step,
                    y: size.height - CGFloat(max(0, value) / top) * size.height)
        }
    }

    private func line(_ pts: [CGPoint]) -> Path {
        var path = Path()
        path.addLines(pts)
        return path
    }

    private func area(_ pts: [CGPoint], height: CGFloat) -> Path {
        var path = line(pts)
        if let first = pts.first, let last = pts.last {
            path.addLine(to: CGPoint(x: last.x, y: height))
            path.addLine(to: CGPoint(x: first.x, y: height))
            path.closeSubpath()
        }
        return path
    }
}

struct ControlsSection: View {
    @ObservedObject var model: TetherModel

    var body: some View {
        VStack(spacing: 12) {
            mainButton
                .controlSize(.large)
                .frame(maxWidth: .infinity)

            HStack(alignment: .center) {
                VStack(alignment: .leading, spacing: 1) {
                    Text("Auto-connect")
                        .font(.callout)
                    Text("Connect when the phone is plugged in")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Toggle("Auto-connect", isOn: $model.autoConnect)
                    .labelsHidden()
                    .toggleStyle(.switch)
                    .controlSize(.mini)
            }

            if let note = model.loginItemNote {
                HStack(alignment: .top) {
                    Text(note)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    Spacer(minLength: 4)
                    Button("Open Settings") { model.openLoginItemsSettings() }
                        .font(.caption)
                        .buttonStyle(.link)
                }
            }
        }
    }

    /// The one action that fits the current state. Waiting with Auto-connect on
    /// has no button: the Auto-connect switch is the way to stop.
    @ViewBuilder private var mainButton: some View {
        switch model.status {
        case .connected:
            Button(action: model.disconnect) {
                Text("Disconnect").frame(maxWidth: .infinity)
            }
        case .waiting:
            if !model.autoConnect {
                Button(action: model.disconnect) {
                    Text("Stop Waiting").frame(maxWidth: .infinity)
                }
            }
        case .off:
            Button(action: model.connect) {
                Text("Connect").frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
        case .serviceDown:
            EmptyView()
        }
    }
}

enum AboutPanel {
    static let projectURL = URL(string: "https://github.com/ashahinL/usb-tether")!

    /// The standard About panel. It shows the version from Info.plist.
    static func show() {
        let credits = NSMutableAttributedString(
            string: "Use your Android phone's internet over a USB cable.\n",
            attributes: [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.labelColor])
        credits.append(NSAttributedString(
            string: "github.com/ashahinL/usb-tether",
            attributes: [.font: NSFont.systemFont(ofSize: 11), .link: projectURL]))
        let paragraph = NSMutableParagraphStyle()
        paragraph.alignment = .center
        credits.addAttribute(.paragraphStyle, value: paragraph, range: NSRange(location: 0, length: credits.length))

        NSApp.activate(ignoringOtherApps: true)
        NSApp.orderFrontStandardAboutPanel(options: [.credits: credits])
    }
}
