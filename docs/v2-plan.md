# USB Tether v2 ideas

Status: backlog. Nothing here is scheduled. Collected after v1.0.0 work, 2026-11-16 (Cairo).

## UI (review of round 2, scored 8 or more in every area)

- The Disconnect button is large and grey, so it feels heavy. Try a smaller or plainer style.
- "Connected for" restarts when the menu-bar app is reopened during a session. The daemon should send the session start time in its `state` message.
- With Auto-connect on, Disconnect sends `stop`, and the daemon reconnects straight away if the phone is still plugged in. Decide what Disconnect should mean then: pause until the phone is unplugged, or turn Auto-connect off.
- Check the gear menu, the popover material over a real wallpaper, and Open at Login on macOS 13, 14 and 15.
- Shrink the README screenshots (230–490 KB each) and `AppIcon.icns` (1.3 MB) with a PNG quantizer.

## Product

- Sign and notarize with an Apple Developer ID so users skip the "Open anyway" step.
- Decide on a VID/PID allowlist or a "trusted phones" list. It does not stop a spoofing device, but it stops the app from claiming every RNDIS device.
- The Rust rewrite is parked on the `rust-rewrite` branch. Revisit only if the C daemon shows real problems.

## Tooling

- Write the `macos-offscreen-ui-render` skill: render SwiftUI/AppKit views to light and dark PNGs off-screen with active control colours and real materials, plus an icon contact sheet at 1024, 128, 32 and 16 px. It came up in the v1 UI work and will recur in any macOS app.
