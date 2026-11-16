// Renders the README popover screenshots from the real SwiftUI views with sample data.
// The sample data lives only here; the shipped app has no demo mode.
// Run `make screenshots` from the repo root. It compiles this file with ui/*.swift
// (except ui/App.swift) and writes PNGs to docs/images.

import AppKit
import SwiftUI

@main
enum RenderScreenshots {
    static func main() throws {
        let args = CommandLine.arguments
        guard args.count == 2 else {
            FileHandle.standardError.write("usage: render-screenshots <output dir>\n".data(using: .utf8)!)
            exit(1)
        }
        let outDir = URL(fileURLWithPath: args[1])
        try FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)

        let app = ActiveApplication.shared
        app.setActivationPolicy(.accessory)
        app.applicationIconImage = NSImage(contentsOfFile: "resources/AppIcon.icns")
        let plist = NSDictionary(contentsOfFile: "resources/Info.plist")
        let version = plist?["CFBundleShortVersionString"] as? String ?? ""

        let scenes: [(String, () -> TetherModel)] = [
            ("connected", connectedModel),
            ("waiting", waitingModel),
            ("off", offModel),
        ]
        for (name, makeModel) in scenes {
            for dark in [false, true] {
                let file = outDir.appendingPathComponent("popover-\(name)\(dark ? "-dark" : "").png")
                try render(PopoverView(model: makeModel(), version: version), dark: dark, to: file)
                print("Wrote \(file.path)")
            }
        }
        UserDefaults.standard.removePersistentDomain(forName: ProcessInfo.processInfo.processName)
    }

    static func connectedModel() -> TetherModel {
        let model = TetherModel()
        model.status = .connected
        model.ip = "192.168.42.129"
        model.iface = "utun5"
        model.stats = Stats(rxMbps: 48.6, txMbps: 6.2, rxBytes: 1_284_000_000, txBytes: 96_400_000)
        model.history = (0..<TetherModel.historyLength).map { i in
            let t = Double(i)
            return SpeedSample(rxMbps: 34 + 12 * sin(t / 6) + 6 * sin(t / 2.3),
                               txMbps: 5 + 2 * sin(t / 4 + 1))
        }
        model.now = Date()
        model.connectedSince = model.now.addingTimeInterval(-(72 * 60 + 14))
        model.openAtLogin = true
        return model
    }

    static func waitingModel() -> TetherModel {
        let model = TetherModel()
        model.status = .waiting
        model.openAtLogin = true
        return model
    }

    static func offModel() -> TetherModel {
        let model = TetherModel()
        model.autoConnect = false
        model.status = .off
        model.openAtLogin = true
        return model
    }

    /// Draws the view on a popover-like card and saves it as a 2x PNG.
    static func render(_ view: PopoverView, dark: Bool, to file: URL) throws {
        let appearance = NSAppearance(named: dark ? .darkAqua : .aqua)!
        let host = NSHostingView(rootView: Card(content: view).environment(\.colorScheme, dark ? .dark : .light))
        host.appearance = appearance
        host.frame = NSRect(origin: .zero, size: host.fittingSize)

        let window = KeyWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
        window.appearance = appearance
        window.backgroundColor = .clear
        window.isOpaque = false
        window.contentView = host
        window.setFrameOrigin(NSPoint(x: -10_000, y: -10_000))
        window.makeKeyAndOrderFront(nil)
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        host.layoutSubtreeIfNeeded()
        host.display()

        let scale: CGFloat = 2
        let size = host.bounds.size
        let rep = NSBitmapImageRep(bitmapDataPlanes: nil,
                                   pixelsWide: Int(size.width * scale), pixelsHigh: Int(size.height * scale),
                                   bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                   colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        rep.size = size
        appearance.performAsCurrentDrawingAppearance {
            host.cacheDisplay(in: host.bounds, to: rep)
        }
        guard let png = rep.representation(using: .png, properties: [:]) else {
            throw CocoaError(.fileWriteUnknown)
        }
        try png.write(to: file)
        window.orderOut(nil)
    }
}

/// Controls draw in their active colours only in the key window of an active app.
/// A command-line tool cannot activate itself, so these say they are.
final class ActiveApplication: NSApplication {
    override var isActive: Bool { true }
}

final class KeyWindow: NSWindow {
    override var canBecomeKey: Bool { true }
    override var isKeyWindow: Bool { true }
}

/// A rounded panel like the menu-bar popover, with room around it for the shadow.
struct Card: View {
    let content: PopoverView

    var body: some View {
        content
            .background(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .fill(Color(nsColor: .windowBackgroundColor))
                    .shadow(color: .black.opacity(0.25), radius: 14, y: 6))
            .overlay(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(Color(nsColor: .separatorColor), lineWidth: 0.5))
            .padding(28)
    }
}
