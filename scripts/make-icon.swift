// Draws the USB Tether app icon and writes an .iconset folder.
// Usage: swift scripts/make-icon.swift build/AppIcon.iconset
//        iconutil -c icns build/AppIcon.iconset -o resources/AppIcon.icns
// Or run `make icon`, which does both.
//
// The picture: a phone with full mobile-signal bars on its screen, and a USB cable
// from the phone ending in a plug that points out of the tile, toward the Mac.
// The tile fades from green (the phone) to blue (the Mac).
// Sizes of 32 px and below get a simpler drawing with bigger, bolder shapes.

import AppKit

func color(_ hex: UInt32, _ alpha: CGFloat = 1) -> NSColor {
    NSColor(srgbRed: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: alpha)
}

let tileGreen = color(0x2BC46E)
let tileBlue = color(0x1D5FD1)
let phoneWhite = color(0xFFFFFF)
let phoneEdge = color(0xDCE6EE)
let screenTop = color(0x123642)
let screenBottom = color(0x08161F)
let barTop = color(0x8CF7B4)
let barBottom = color(0x34D67F)
let metalLight = color(0xF1F4F7)
let metalDark = color(0xA9B6C2)

/// The macOS icon tile: an 824 pt rounded square with continuous corners, centred on the 1024 canvas.
func tilePath() -> NSBezierPath {
    let rect = NSRect(x: 100, y: 100, width: 824, height: 824)
    let path = NSBezierPath()
    let steps = 720
    let a = rect.width / 2, b = rect.height / 2
    let n: CGFloat = 5
    for i in 0..<steps {
        let t = CGFloat(i) / CGFloat(steps) * 2 * .pi
        let c = cos(t), s = sin(t)
        let x = pow(abs(c), 2 / n) * a * (c < 0 ? -1 : 1)
        let y = pow(abs(s), 2 / n) * b * (s < 0 ? -1 : 1)
        let point = NSPoint(x: rect.midX + x, y: rect.midY + y)
        if i == 0 { path.move(to: point) } else { path.line(to: point) }
    }
    path.close()
    return path
}

func withShadow(blur: CGFloat, offsetY: CGFloat, alpha: CGFloat, _ body: () -> Void) {
    NSGraphicsContext.saveGraphicsState()
    let shadow = NSShadow()
    shadow.shadowColor = NSColor.black.withAlphaComponent(alpha)
    shadow.shadowBlurRadius = blur
    shadow.shadowOffset = NSSize(width: 0, height: offsetY)
    shadow.set()
    body()
    NSGraphicsContext.restoreGraphicsState()
}

/// Layout numbers for one level of detail.
struct Style {
    var phone: NSRect
    var phoneRadius: CGFloat
    var tilt: CGFloat
    var bezel: CGFloat
    var bars: Int
    var barWidth: CGFloat
    var barGap: CGFloat
    var barBase: CGFloat
    var camera: Bool
    var cableWidth: CGFloat
    var cableDrop: CGFloat
    var plug: NSRect
    var tip: NSRect
    var shadowScale: CGFloat

    static let detailed = Style(
        phone: NSRect(x: 262, y: 356, width: 296, height: 486), phoneRadius: 62, tilt: -9, bezel: 20,
        bars: 4, barWidth: 38, barGap: 17, barBase: 110, camera: true,
        cableWidth: 46, cableDrop: 230,
        plug: NSRect(x: 590, y: 184, width: 184, height: 112), tip: NSRect(x: 760, y: 208, width: 92, height: 64),
        shadowScale: 1)

    static let small = Style(
        phone: NSRect(x: 270, y: 330, width: 340, height: 520), phoneRadius: 70, tilt: 0, bezel: 34,
        bars: 3, barWidth: 62, barGap: 30, barBase: 90, camera: false,
        cableWidth: 72, cableDrop: 250,
        plug: NSRect(x: 600, y: 170, width: 170, height: 120), tip: NSRect(x: 760, y: 194, width: 90, height: 72),
        shadowScale: 0.6)
}

func drawIcon(_ style: Style) {
    let tile = tilePath()

    // Tile with its drop shadow, green to blue, and a soft light from the top.
    withShadow(blur: 24, offsetY: -10, alpha: 0.3) {
        tileBlue.setFill()
        tile.fill()
    }
    NSGradient(starting: tileGreen, ending: tileBlue)!.draw(in: tile, angle: -60)
    NSGraphicsContext.saveGraphicsState()
    tile.addClip()
    NSGradient(colors: [color(0xFFFFFF, 0.22), color(0xFFFFFF, 0)])!
        .draw(in: NSRect(x: 100, y: 512, width: 824, height: 412), angle: -90)
    NSGraphicsContext.restoreGraphicsState()

    // Where the cable leaves the phone, after the tilt.
    let phone = style.phone
    let center = NSPoint(x: phone.midX, y: phone.midY)
    let tilt = AffineTransform(rotationByDegrees: style.tilt)
    let exitOffset = tilt.transform(NSPoint(x: 0, y: -phone.height / 2 + style.phoneRadius))
    let exit = NSPoint(x: center.x + exitOffset.x, y: center.y + exitOffset.y)

    // Cable: down out of the phone, then a smooth turn right into the plug.
    let plugY = style.plug.midY
    let cable = NSBezierPath()
    cable.move(to: exit)
    cable.curve(to: NSPoint(x: exit.x + 150, y: plugY),
                controlPoint1: NSPoint(x: exit.x - 10, y: exit.y - style.cableDrop * 0.75),
                controlPoint2: NSPoint(x: exit.x + 20, y: plugY))
    cable.line(to: NSPoint(x: style.plug.minX + 20, y: plugY))
    cable.lineWidth = style.cableWidth
    cable.lineCapStyle = .round
    cable.lineJoinStyle = .round

    withShadow(blur: 26 * style.shadowScale, offsetY: -12 * style.shadowScale, alpha: 0.28) {
        phoneWhite.setStroke()
        cable.stroke()
        // Plug body, then the metal USB-C tip pointing out of the tile.
        phoneWhite.setFill()
        let radius = style.plug.height * 0.28
        NSBezierPath(roundedRect: style.plug, xRadius: radius, yRadius: radius).fill()
    }
    let tipRadius = style.tip.height * 0.32
    let tipPath = NSBezierPath(roundedRect: style.tip, xRadius: tipRadius, yRadius: tipRadius)
    NSGradient(starting: metalLight, ending: metalDark)!.draw(in: tipPath, angle: -90)
    if style.camera {
        // The slot in the USB-C tip.
        let slot = style.tip.insetBy(dx: 18, dy: style.tip.height * 0.37)
        color(0x5D6B78).setFill()
        NSBezierPath(roundedRect: slot, xRadius: slot.height / 2, yRadius: slot.height / 2).fill()
        // A grip line across the plug body.
        let grip = NSBezierPath()
        grip.move(to: NSPoint(x: style.plug.maxX - 48, y: style.plug.minY + 26))
        grip.line(to: NSPoint(x: style.plug.maxX - 48, y: style.plug.maxY - 26))
        grip.lineWidth = 7
        grip.lineCapStyle = .round
        phoneEdge.setStroke()
        grip.stroke()
    }

    // Phone, tilted a little, drawn around its centre.
    NSGraphicsContext.saveGraphicsState()
    let move = NSAffineTransform()
    move.translateX(by: center.x, yBy: center.y)
    move.rotate(byDegrees: style.tilt)
    move.concat()
    let body = NSRect(x: -phone.width / 2, y: -phone.height / 2, width: phone.width, height: phone.height)

    withShadow(blur: 34 * style.shadowScale, offsetY: -16 * style.shadowScale, alpha: 0.32) {
        phoneWhite.setFill()
        NSBezierPath(roundedRect: body, xRadius: style.phoneRadius, yRadius: style.phoneRadius).fill()
    }
    let edge = NSBezierPath(roundedRect: body.insetBy(dx: 3, dy: 3),
                            xRadius: style.phoneRadius - 3, yRadius: style.phoneRadius - 3)
    edge.lineWidth = 6
    phoneEdge.setStroke()
    edge.stroke()

    let screen = body.insetBy(dx: style.bezel, dy: style.bezel)
    let screenRadius = style.phoneRadius - style.bezel * 0.6
    let screenPath = NSBezierPath(roundedRect: screen, xRadius: screenRadius, yRadius: screenRadius)
    NSGradient(starting: screenTop, ending: screenBottom)!.draw(in: screenPath, angle: -90)

    if style.camera {
        // Punch-hole camera, as on most Android phones.
        let dot = NSRect(x: -11, y: screen.maxY - 44, width: 22, height: 22)
        color(0x000000, 0.85).setFill()
        NSBezierPath(ovalIn: dot).fill()
    }

    // Mobile-signal bars, all lit: the phone has internet to share.
    let total = CGFloat(style.bars) * style.barWidth + CGFloat(style.bars - 1) * style.barGap
    let tallest = screen.height - style.barBase - (style.camera ? 120 : 96)
    let base = screen.minY + style.barBase
    for i in 0..<style.bars {
        let height = tallest * CGFloat(i + 1) / CGFloat(style.bars)
        let x = -total / 2 + CGFloat(i) * (style.barWidth + style.barGap)
        let bar = NSRect(x: x, y: base, width: style.barWidth, height: height)
        let radius = min(style.barWidth * 0.3, 14)
        let path = NSBezierPath(roundedRect: bar, xRadius: radius, yRadius: radius)
        NSGraphicsContext.saveGraphicsState()
        let glow = NSShadow()
        glow.shadowColor = barBottom.withAlphaComponent(0.55)
        glow.shadowBlurRadius = 22 * style.shadowScale
        glow.set()
        barBottom.setFill()
        path.fill()
        NSGraphicsContext.restoreGraphicsState()
        NSGradient(starting: barTop, ending: barBottom)!.draw(in: path, angle: -90)
    }
    NSGraphicsContext.restoreGraphicsState()
}

func renderPNG(pixels: Int) -> Data {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels,
                               bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    rep.size = NSSize(width: pixels, height: pixels)
    NSGraphicsContext.saveGraphicsState()
    let context = NSGraphicsContext(bitmapImageRep: rep)!
    NSGraphicsContext.current = context
    context.imageInterpolation = .high
    let scale = CGFloat(pixels) / 1024
    context.cgContext.scaleBy(x: scale, y: scale)
    drawIcon(pixels <= 32 ? .small : .detailed)
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

let args = CommandLine.arguments
guard args.count == 2 else {
    FileHandle.standardError.write("usage: swift scripts/make-icon.swift <output.iconset>\n".data(using: .utf8)!)
    exit(1)
}
let outDir = URL(fileURLWithPath: args[1])
try FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)

for points in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let name = scale == 1 ? "icon_\(points)x\(points).png" : "icon_\(points)x\(points)@2x.png"
        try renderPNG(pixels: points * scale).write(to: outDir.appendingPathComponent(name))
    }
}
print("Wrote \(outDir.path)")
