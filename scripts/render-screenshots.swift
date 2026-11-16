// Renders the README popover screenshots from the real SwiftUI views with sample data,
// on the real popover material (NSVisualEffectView) over a plain desktop.
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

        let scenes: [(String, () -> TetherModel)] = [
            ("connected", connectedModel),
            ("waiting", waitingModel),
            ("off", offModel),
            ("service-down", serviceDownModel),
        ]
        for (name, makeModel) in scenes {
            for dark in [false, true] {
                let file = outDir.appendingPathComponent("popover-\(name)\(dark ? "-dark" : "").png")
                try render(PopoverView(model: makeModel()), dark: dark, to: file)
                print("Wrote \(file.path)")
            }
        }
        UserDefaults.standard.removePersistentDomain(forName: ProcessInfo.processInfo.processName)
    }

    static func connectedModel() -> TetherModel {
        let model = TetherModel()
        model.autoConnect = true
        model.status = .connected
        model.ip = "192.168.42.129"
        model.iface = "utun5"
        // A plausible minute: a busy download with a page-load burst, and a small, spiky upload.
        model.history = (0..<TetherModel.historyLength).map { i in
            let t = Double(i)
            let burst = (36...43).contains(i) ? 26.0 * sin(Double(i - 35) / 9 * .pi) : 0
            let down = 30 + 6 * sin(t / 7) + 1.8 * sin(t * 1.3) + burst
            let up = 2.4 + 0.8 * sin(t / 5 + 1) + 0.3 * sin(t * 1.9) + ([12, 13, 37, 38, 39].contains(i) ? 3.2 : 0)
            return SpeedSample(rxMbps: max(0, down), txMbps: max(0, up))
        }
        let last = model.history.last!
        model.stats = Stats(rxMbps: last.rxMbps, txMbps: last.txMbps,
                            rxBytes: 1_284_000_000, txBytes: 96_400_000)
        model.now = Date()
        model.connectedSince = model.now.addingTimeInterval(-(72 * 60 + 14))
        model.openAtLogin = true
        return model
    }

    static func waitingModel() -> TetherModel {
        let model = TetherModel()
        model.autoConnect = true
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

    static func serviceDownModel() -> TetherModel {
        let model = TetherModel()
        model.autoConnect = true
        model.status = .serviceDown
        model.openAtLogin = true
        return model
    }

    /// Draws the view on the real popover material over a plain desktop and saves it as a 2x PNG.
    static func render(_ view: PopoverView, dark: Bool, to file: URL) throws {
        let appearance = NSAppearance(named: dark ? .darkAqua : .aqua)!
        let host = NSHostingView(rootView: view.environment(\.colorScheme, dark ? .dark : .light))
        host.appearance = appearance
        let card = NSRect(origin: NSPoint(x: Desktop.margin, y: Desktop.margin), size: host.fittingSize)

        let desktop = Desktop(card: card, dark: dark)
        let material = NSVisualEffectView(frame: card)
        material.material = .popover
        material.blendingMode = .withinWindow
        material.state = .active
        material.wantsLayer = true
        material.layer?.cornerRadius = Desktop.radius
        material.layer?.cornerCurve = .continuous
        material.layer?.masksToBounds = true
        host.frame = material.bounds
        material.addSubview(host)
        desktop.addSubview(material)
        let edge = Edge(frame: card)
        desktop.addSubview(edge)

        let window = KeyWindow(contentRect: desktop.frame, styleMask: [.borderless], backing: .buffered, defer: false)
        window.appearance = appearance
        window.contentView = desktop
        window.setFrameOrigin(NSPoint(x: -10_000, y: -10_000))
        window.makeKeyAndOrderFront(nil)
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        desktop.layoutSubtreeIfNeeded()
        desktop.display()

        let scale: CGFloat = 2
        let size = desktop.bounds.size
        let rep = NSBitmapImageRep(bitmapDataPlanes: nil,
                                   pixelsWide: Int(size.width * scale), pixelsHigh: Int(size.height * scale),
                                   bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                   colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        rep.size = size
        appearance.performAsCurrentDrawingAppearance {
            desktop.cacheDisplay(in: desktop.bounds, to: rep)
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

/// A plain desktop behind the popover, with the popover's drop shadow.
/// The popover material tints whatever is behind it, so the backdrop is kept soft.
final class Desktop: NSView {
    static let margin: CGFloat = 30
    static let radius: CGFloat = 12

    private let card: NSRect
    private let dark: Bool

    init(card: NSRect, dark: Bool) {
        self.card = card
        self.dark = dark
        super.init(frame: NSRect(x: 0, y: 0, width: card.width + 2 * Self.margin,
                                 height: card.height + 2 * Self.margin))
    }

    required init?(coder: NSCoder) { nil }

    override func draw(_ dirtyRect: NSRect) {
        let colors = dark
            ? [NSColor(srgbRed: 0.13, green: 0.16, blue: 0.23, alpha: 1), NSColor(srgbRed: 0.20, green: 0.15, blue: 0.22, alpha: 1)]
            : [NSColor(srgbRed: 0.80, green: 0.86, blue: 0.93, alpha: 1), NSColor(srgbRed: 0.93, green: 0.88, blue: 0.84, alpha: 1)]
        NSGradient(colors: colors)!.draw(in: bounds, angle: -60)

        // Shadow only: clip the card out so the material is not darkened from below.
        let cardPath = NSBezierPath(roundedRect: card, xRadius: Self.radius, yRadius: Self.radius)
        NSGraphicsContext.saveGraphicsState()
        let outside = NSBezierPath(rect: bounds)
        outside.append(cardPath)
        outside.windingRule = .evenOdd
        outside.addClip()
        let shadow = NSShadow()
        shadow.shadowColor = NSColor.black.withAlphaComponent(dark ? 0.5 : 0.22)
        shadow.shadowBlurRadius = 16
        shadow.shadowOffset = NSSize(width: 0, height: -6)
        shadow.set()
        NSColor.black.setFill()
        cardPath.fill()
        NSGraphicsContext.restoreGraphicsState()
    }
}

/// The thin outline that popovers draw around their edge.
final class Edge: NSView {
    override func draw(_ dirtyRect: NSRect) {
        let path = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.25, dy: 0.25),
                                xRadius: Desktop.radius, yRadius: Desktop.radius)
        path.lineWidth = 0.5
        let dark = effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        (dark ? NSColor.white.withAlphaComponent(0.18) : NSColor.black.withAlphaComponent(0.14)).setStroke()
        path.stroke()
    }
}
