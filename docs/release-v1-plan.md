# USB Tether v1.0.0 release

Status: in progress on branch `ship-v1`.

Ship the C app as a product named **USB Tether**, version **1.0.0**, as a `.pkg` inside a `.dmg` on a GitHub release. The Rust rewrite stays parked on `dev`.

## Decisions

- Display name: **USB Tether**. App bundle: `/Applications/USB Tether.app`. Bundle IDs (`com.ashahin.macostether`, `com.ashahin.macostetherui`), the daemon binary `macostether`, the socket and log paths stay the same, so upgrades replace the old install.
- Version 1.0.0. The changelog keeps the original project's history under a "Before 1.0.0" note.
- Unsigned for v1. The build accepts `SIGN_IDENTITY` and a notarize step later. The README explains the one-time "Open anyway" step.
- Download: `USB-Tether-1.0.0.dmg` that holds `USB Tether.pkg` and `Uninstall USB Tether.command`. A `.pkg` is required because the background service is installed as root.
- Minimum macOS 13 (Ventura). The new UI uses SwiftUI and `SMAppService` for Open at Login. macOS 13 is the oldest version Apple still patches.
- MIT license keeps Hakan Saglam's original notice next to Ammar's. The README credits the original project.
- Universal binary (arm64 + x86_64) with libusb built from a pinned, checksummed source release and linked statically.

## Tasks

1. Done. License and credit. Restore the original copyright line, add Ammar's, credit the original project in the README. Owner: Claude.
2. Done. Rename to USB Tether and set version 1.0.0. Info.plist, Makefile app path, LaunchDaemon `ProgramArguments`, pkg scripts, install.sh, release workflow, UI strings. Preinstall removes the old `macOs_tether.app`. Owner: Claude.
3. Done. New menu-bar UI. SwiftUI popover from a template menu-bar icon: connection state, phone IP, live down/up speed, session data, duration, Connect/Disconnect, Auto-connect, Open at Login, About, Quit. Clear states for "service not running", "waiting for phone", "connected", "error". App icon (`.icns`). Lane: `art` (Claude Opus 5.5, high).
   - Built as an `NSStatusItem` with an `NSPopover` that hosts SwiftUI. `ui/MenuApp.swift` is split into `ui/App.swift`, `IPCClient.swift`, `TetherModel.swift` and `Views.swift`. The Makefile compiles `ui/*.swift`.
   - "Error" is the "Service not running" state. The daemon sends no other error state.
   - Auto-connect is on by default for new installs. A "Show speed in menu bar" switch was added.
   - The daemon now sends the current state to each new UI client (`src/ipc.c`).
   - The icon is drawn by `scripts/make-icon.swift`. `make icon` rebuilds `resources/AppIcon.icns`.
   - `make screenshots` renders `docs/images/popover-*.png`, light and dark (`-dark`). It draws the real views in an off-screen `NSHostingView`, not with `ImageRenderer`, because `ImageRenderer` cannot draw switches on macOS.
   - Round 2 (review scored round 1 at 7/10; target 8+ in every area):
     - The popover draws no background, so the system popover material shows through. The screenshots draw the real `NSVisualEffectView` popover material over a plain desktop.
     - Each speed tile has its own graph of the last 60 seconds, with its own scale, straight lines between samples (no smoothing), a dashed peak line with a "Peak" label, and a dot on the newest sample.
     - Upload is orange everywhere (tile arrow, graph, session data). Download stays blue.
     - The IP row is now "IP address" under the session rows. That IP is the Mac's address on the phone's network. The interface name (`utun5`) is only in its tooltip.
     - Only Auto-connect stays in the main view, with a one-line explanation. "Open at Login", "Show Speed in Menu Bar", About and Quit moved to a gear menu in the header. The footer and its version text are gone; the version shows in the About panel.
     - Main button by state: Connected "Disconnect", Off "Connect", Waiting with Auto-connect off "Stop Waiting" (sends `disable`). Waiting with Auto-connect on and Service not running have no button, so no disabled button repeats the status.
     - New icon: a tilted phone with lit signal bars and a punch-hole camera, a USB-C cable and plug, on a green-to-blue tile. Sizes of 32 px and below use a simpler, bolder drawing.
     - `make screenshots` also renders the "Service not running" state (`popover-service-down*.png`).
   - Known limits: the connected time counts from when the UI first sees the connection, so it restarts if the UI is reopened mid-session. With Auto-connect on, Disconnect sends `stop`, and the daemon reconnects at once if the phone is still plugged in with tethering on.
4. Done. Packaging and CI. `make dist` builds libusb 1.0.30 from its checksummed official source tarball, links it statically into a universal daemon, builds the Swift UI per architecture, and packages the app as `USB Tether.pkg` inside `USB-Tether-1.0.0.dmg` with an executable uninstaller and README. Versions come from Info.plist. Optional app and installer signing remain available. Parent review and commits are pending.
   - Release outputs use `build/release/`, separate from the Homebrew development build. `build/deps/` caches the universal static library and headers by version. libusb uses its `pipe` fallback because newer SDKs expose `pipe2`, which requires macOS 27.
   - Installer scripts use `launchctl bootout` and `bootstrap`, quit the console user's UI, remove both app names on upgrade, and open the UI for the logged-in user. The uninstaller removes the service, app, socket, log, and receipt.
   - `tests/test_dist.sh` checks architectures, dynamic libraries, package payload and distribution requirements, mounted disk-image contents, checksum, shell scripts, and workflow YAML. `tests/test_release.sh` runs a clean dev build, parser tests, a release build with Homebrew excluded from PATH, and artifact checks.
   - A full source dependency build and artifact checks passed locally. Installing and uninstalling the package are reserved for task 7. Unsigned builds and CI publication still need the parent's release checks.
5. Done. CI was folded into task 4. Pull requests and pushes to `main` and `ship-v1` build the dev app, run `make test`, build the disk image, and verify artifacts. Tags `v*` must match Info.plist, build and verify the disk image, then publish it with its checksum and the matching CHANGELOG section when present. Checkout is pinned to a full commit SHA. Only the release job has write permission.
6. Done. Repo polish. README rewritten for users first (screenshot, install, first-run Gatekeeper step, uninstall, troubleshooting, credits), CHANGELOG 1.0.0, CONTRIBUTING update, SECURITY.md, issue and PR templates, repo description and topics. Owner: Claude, with the `second` lane for the templates if needed.
7. Release candidate. Build the `.dmg` locally. Ammar installs it and tests on the phone: connect, browse, disconnect, unplug, replug, reboot, uninstall. Owner: Ammar.
8. Release. Merge `ship-v1` into `main`, tag `v1.0.0`, push, let CI publish, download the `.dmg` from the release page and check its checksum. Owner: Claude, after Ammar's go-ahead.

## Checks

- `make` and `make test` pass.
- `make dist` builds the `.dmg`, and `lipo -archs` shows `arm64 x86_64` for both binaries. `otool -L` shows no Homebrew libusb.
- The release candidate passes task 7 on the phone.
