import AppKit
import Combine
import SwiftUI

@main
enum USBTetherApp {
    static func main() {
        let app = NSApplication.shared
        let delegate = AppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.accessory)
        app.run()
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    private let model = TetherModel()
    private var statusItem: NSStatusItem!
    private let popover = NSPopover()
    private var changes: AnyCancellable?

    func applicationDidFinishLaunching(_ notification: Notification) {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        if let button = statusItem.button {
            button.imagePosition = .imageLeading
            button.target = self
            button.action = #selector(togglePopover)
        }

        // No background view here: the popover's own translucent material shows behind the content.
        let host = NSHostingController(rootView: PopoverView(model: model))
        host.sizingOptions = .preferredContentSize
        popover.contentViewController = host
        popover.behavior = .transient
        popover.animates = true

        changes = model.objectWillChange
            .receive(on: RunLoop.main)
            .sink { [weak self] in self?.updateStatusItem() }

        model.start()
        updateStatusItem()
    }

    func applicationWillTerminate(_ notification: Notification) {
        model.stop()
    }

    @objc private func togglePopover() {
        guard let button = statusItem.button else { return }
        if popover.isShown {
            popover.performClose(nil)
        } else {
            model.refreshLoginItem()
            NSApp.activate(ignoringOtherApps: true)
            popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        }
    }

    private var lastLoggedStatus: TetherStatus?

    private func updateStatusItem() {
        guard let button = statusItem.button else { return }
        button.image = StatusIcon.image(for: model.status)
        button.toolTip = "USB Tether: \(model.status.title)"

        if model.status == .connected, model.showSpeedInMenuBar, let stats = model.stats {
            button.attributedTitle = NSAttributedString(
                string: " " + Format.speed(stats.rxMbps),
                attributes: [.font: NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .regular)])
        } else {
            button.title = ""
        }

        if lastLoggedStatus != model.status {
            lastLoggedStatus = model.status
            NSLog("USB Tether status: %@", model.status.title)
        }
    }
}

/// Menu-bar template images built from the `cable.connector` SF Symbol (macOS 12+).
enum StatusIcon {
    private static var cache: [TetherStatus: NSImage] = [:]

    static func image(for status: TetherStatus) -> NSImage? {
        if let cached = cache[status] { return cached }
        let config = NSImage.SymbolConfiguration(pointSize: 15, weight: .regular)
        let symbol = status == .serviceDown ? "exclamationmark.triangle" : "cable.connector"
        guard let base = NSImage(systemSymbolName: symbol, accessibilityDescription: status.title)?
            .withSymbolConfiguration(config) else { return nil }

        let image: NSImage
        switch status {
        case .connected, .serviceDown:
            image = square(base) { rect in base.draw(in: rect) }
        case .waiting:
            image = square(base) { rect in base.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 0.5) }
        case .off:
            image = square(base) { rect in
                base.draw(in: rect)
                slash(across: rect)
            }
        }
        image.isTemplate = true
        image.accessibilityDescription = "USB Tether: \(status.title)"
        cache[status] = image
        return image
    }

    /// Square canvas so every state has the same width in the menu bar; `body` gets the symbol's rect.
    private static func square(_ base: NSImage, _ body: @escaping (NSRect) -> Void) -> NSImage {
        let side = max(base.size.width, base.size.height)
        return NSImage(size: NSSize(width: side, height: side), flipped: false) { _ in
            body(NSRect(x: (side - base.size.width) / 2, y: (side - base.size.height) / 2,
                        width: base.size.width, height: base.size.height))
            return true
        }
    }

    /// A 45 degree line with a clear gap around it, like the system ".slash" symbols.
    private static func slash(across symbol: NSRect) {
        let half = symbol.height / 2 - 1.5
        let from = NSPoint(x: symbol.midX - half, y: symbol.midY + half)
        let to = NSPoint(x: symbol.midX + half, y: symbol.midY - half)
        let gap = NSBezierPath()
        gap.move(to: from)
        gap.line(to: to)
        gap.lineWidth = 4
        gap.lineCapStyle = .round
        NSGraphicsContext.current?.compositingOperation = .clear
        gap.stroke()
        NSGraphicsContext.current?.compositingOperation = .sourceOver
        NSColor.black.setStroke()
        let line = NSBezierPath()
        line.move(to: from)
        line.line(to: to)
        line.lineWidth = 1.5
        line.lineCapStyle = .round
        line.stroke()
    }
}
