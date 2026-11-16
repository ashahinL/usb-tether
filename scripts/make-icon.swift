// Draws the USB Tether app icon and writes an .iconset folder.
// Usage: swift scripts/make-icon.swift build/AppIcon.iconset
//        iconutil -c icns build/AppIcon.iconset -o resources/AppIcon.icns
// Or run `make icon`, which does both.

import AppKit

func color(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ a: CGFloat = 1) -> NSColor {
    NSColor(srgbRed: r / 255, green: g / 255, blue: b / 255, alpha: a)
}

let backgroundTop = color(98, 200, 186)
let backgroundBottom = color(34, 104, 158)
let ink = color(255, 255, 255)
let inkShade = color(226, 238, 243)
let screenTop = color(74, 160, 178)
let screenBottom = color(40, 98, 140)

/// Draws the icon on a 1024 x 1024 canvas (origin bottom left).
func drawIcon() {
    // Rounded-square tile with the standard macOS margins and a soft shadow.
    let tile = NSRect(x: 100, y: 100, width: 824, height: 824)
    let tilePath = NSBezierPath(roundedRect: tile, xRadius: 185, yRadius: 185)
    NSGraphicsContext.saveGraphicsState()
    let shadow = NSShadow()
    shadow.shadowColor = NSColor.black.withAlphaComponent(0.28)
    shadow.shadowBlurRadius = 24
    shadow.shadowOffset = NSSize(width: 0, height: -10)
    shadow.set()
    backgroundBottom.setFill()
    tilePath.fill()
    NSGraphicsContext.restoreGraphicsState()
    NSGradient(starting: backgroundTop, ending: backgroundBottom)!.draw(in: tilePath, angle: -90)

    // Cable from the bottom of the phone, curving right into a USB plug.
    let cable = NSBezierPath()
    cable.move(to: NSPoint(x: 432, y: 400))
    cable.line(to: NSPoint(x: 432, y: 330))
    cable.curve(to: NSPoint(x: 552, y: 230),
                controlPoint1: NSPoint(x: 432, y: 262),
                controlPoint2: NSPoint(x: 480, y: 230))
    cable.line(to: NSPoint(x: 630, y: 230))
    cable.lineWidth = 36
    cable.lineCapStyle = .round
    cable.lineJoinStyle = .round

    NSGraphicsContext.saveGraphicsState()
    let softShadow = NSShadow()
    softShadow.shadowColor = NSColor.black.withAlphaComponent(0.22)
    softShadow.shadowBlurRadius = 18
    softShadow.shadowOffset = NSSize(width: 0, height: -8)
    softShadow.set()

    ink.setStroke()
    cable.stroke()

    // Plug body and metal tip.
    let plug = NSBezierPath(roundedRect: NSRect(x: 612, y: 188, width: 130, height: 84), xRadius: 22, yRadius: 22)
    ink.setFill()
    plug.fill()
    let tip = NSBezierPath(roundedRect: NSRect(x: 728, y: 204, width: 66, height: 52), xRadius: 12, yRadius: 12)
    inkShade.setFill()
    tip.fill()

    // Phone body.
    let body = NSRect(x: 282, y: 380, width: 300, height: 500)
    let bodyPath = NSBezierPath(roundedRect: body, xRadius: 62, yRadius: 62)
    ink.setFill()
    bodyPath.fill()
    NSGraphicsContext.restoreGraphicsState()

    // Screen.
    let screen = body.insetBy(dx: 24, dy: 24)
    let screenPath = NSBezierPath(roundedRect: screen, xRadius: 42, yRadius: 42)
    NSGradient(starting: screenTop, ending: screenBottom)!.draw(in: screenPath, angle: -90)

    // Down and up arrows on the screen: data flowing both ways.
    drawArrow(centerX: 386, top: 730, bottom: 540, pointingDown: true)
    drawArrow(centerX: 478, top: 730, bottom: 540, pointingDown: false)

    // Plug detail line, gives the plug a little depth.
    let seam = NSBezierPath()
    seam.move(to: NSPoint(x: 712, y: 206))
    seam.line(to: NSPoint(x: 712, y: 254))
    seam.lineWidth = 6
    seam.lineCapStyle = .round
    inkShade.setStroke()
    seam.stroke()
}

func drawArrow(centerX: CGFloat, top: CGFloat, bottom: CGFloat, pointingDown: Bool) {
    let head: CGFloat = 46
    let path = NSBezierPath()
    path.move(to: NSPoint(x: centerX, y: top))
    path.line(to: NSPoint(x: centerX, y: bottom))
    let tipY = pointingDown ? bottom : top
    let wingY = pointingDown ? bottom + head : top - head
    path.move(to: NSPoint(x: centerX - head, y: wingY))
    path.line(to: NSPoint(x: centerX, y: tipY))
    path.line(to: NSPoint(x: centerX + head, y: wingY))
    path.lineWidth = 26
    path.lineCapStyle = .round
    path.lineJoinStyle = .round
    ink.setStroke()
    path.stroke()
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
    drawIcon()
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
