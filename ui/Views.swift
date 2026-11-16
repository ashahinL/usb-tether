import AppKit
import SwiftUI

enum Palette {
    static let download = Color(nsColor: .systemBlue)
    static let upload = Color(nsColor: .systemPurple)

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

struct PopoverView: View {
    @ObservedObject var model: TetherModel
    let version: String

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HeaderView(status: model.status)
                .padding(.horizontal, 16)
                .padding(.vertical, 14)
            Divider()
            StatusSection(model: model)
                .padding(16)
            Divider()
            ControlsSection(model: model)
                .padding(16)
            Divider()
            FooterView(version: version)
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
        }
        .frame(width: 320)
    }
}

struct HeaderView: View {
    let status: TetherStatus

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
                        .fill(Palette.dot(status))
                        .frame(width: 8, height: 8)
                    Text(status.title)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer()
        }
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
            HStack {
                Text("Phone")
                    .foregroundStyle(.secondary)
                Text(model.ip ?? "-")
                    .font(.callout.monospacedDigit())
                    .textSelection(.enabled)
                Spacer()
                Text(model.iface ?? "")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            .font(.callout)

            HStack(spacing: 10) {
                SpeedTile(title: "Download", symbol: "arrow.down", tint: Palette.download,
                          mbps: model.stats?.rxMbps ?? 0)
                SpeedTile(title: "Upload", symbol: "arrow.up", tint: Palette.upload,
                          mbps: model.stats?.txMbps ?? 0)
            }

            Sparkline(samples: model.history, capacity: TetherModel.historyLength)
                .frame(height: 44)

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
            }
        }
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

struct SpeedTile: View {
    let title: String
    let symbol: String
    let tint: Color
    let mbps: Double

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
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
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.05)))
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

/// Download (filled) and upload (line) speed over the last `capacity` samples, newest on the right.
struct Sparkline: View {
    let samples: [SpeedSample]
    let capacity: Int

    var body: some View {
        GeometryReader { geo in
            let peak = max(1, samples.map { max($0.rxMbps, $0.txMbps) }.max() ?? 1)
            let down = points(samples.map(\.rxMbps), peak: peak, size: geo.size)
            let up = points(samples.map(\.txMbps), peak: peak, size: geo.size)
            ZStack {
                RoundedRectangle(cornerRadius: 6).fill(Color.primary.opacity(0.04))
                if down.count > 1 {
                    area(down, height: geo.size.height)
                        .fill(LinearGradient(colors: [Palette.download.opacity(0.35), Palette.download.opacity(0.03)],
                                             startPoint: .top, endPoint: .bottom))
                    line(down).stroke(Palette.download, style: StrokeStyle(lineWidth: 1.5, lineJoin: .round))
                    line(up).stroke(Palette.upload, style: StrokeStyle(lineWidth: 1.5, lineJoin: .round))
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 6))
        }
        .accessibilityLabel("Speed over the last minute")
    }

    private func points(_ values: [Double], peak: Double, size: CGSize) -> [CGPoint] {
        let step = size.width / CGFloat(max(1, capacity - 1))
        let offset = capacity - values.count
        let usable = size.height - 4
        return values.enumerated().map { index, value in
            CGPoint(x: CGFloat(offset + index) * step,
                    y: size.height - 2 - CGFloat(value / peak) * usable)
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
        VStack(spacing: 10) {
            mainButton
                .controlSize(.large)
                .frame(maxWidth: .infinity)

            VStack(spacing: 8) {
                SwitchRow(title: "Auto-connect", isOn: $model.autoConnect)
                SwitchRow(title: "Open at Login", isOn: Binding(
                    get: { model.openAtLogin },
                    set: { model.setOpenAtLogin($0) }))
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
                SwitchRow(title: "Show speed in menu bar", isOn: $model.showSpeedInMenuBar)
            }
        }
    }

    @ViewBuilder private var mainButton: some View {
        switch model.status {
        case .connected:
            Button(action: model.disconnect) {
                Text("Disconnect").frame(maxWidth: .infinity)
            }
        case .waiting:
            Button(action: model.disconnect) {
                Text(model.autoConnect ? "Waiting for phone" : "Stop waiting").frame(maxWidth: .infinity)
            }
            .disabled(model.autoConnect)
        case .off, .serviceDown:
            Button(action: model.connect) {
                Text("Connect").frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .disabled(model.status == .serviceDown)
        }
    }
}

struct SwitchRow: View {
    let title: String
    @Binding var isOn: Bool

    var body: some View {
        HStack {
            Text(title)
            Spacer()
            Toggle(title, isOn: $isOn)
                .labelsHidden()
                .toggleStyle(.switch)
                .controlSize(.mini)
        }
        .font(.callout)
    }
}

struct FooterView: View {
    let version: String

    var body: some View {
        HStack {
            Button("About") { AboutPanel.show() }
            Spacer()
            Text("Version \(version)")
                .font(.caption)
                .foregroundStyle(.tertiary)
            Spacer()
            Button("Quit") { NSApp.terminate(nil) }
        }
        .buttonStyle(.borderless)
        .font(.callout)
    }
}

enum AboutPanel {
    static let projectURL = URL(string: "https://github.com/ashahinL/macOs_tether")!

    static func show() {
        let credits = NSMutableAttributedString(
            string: "Use your Android phone's internet over a USB cable.\n",
            attributes: [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.labelColor])
        credits.append(NSAttributedString(
            string: "github.com/ashahinL/macOs_tether",
            attributes: [.font: NSFont.systemFont(ofSize: 11), .link: projectURL]))
        let paragraph = NSMutableParagraphStyle()
        paragraph.alignment = .center
        credits.addAttribute(.paragraphStyle, value: paragraph, range: NSRange(location: 0, length: credits.length))

        NSApp.activate(ignoringOtherApps: true)
        NSApp.orderFrontStandardAboutPanel(options: [.credits: credits])
    }
}
