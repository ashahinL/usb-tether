# Contributing to USB Tether

Thank you for your interest in contributing! This guide will help you get started.

## Development Setup

### Prerequisites

- macOS 13 (Ventura) or later
- Xcode Command Line Tools (`xcode-select --install`)
- [Homebrew](https://brew.sh)
- libusb (`brew install libusb`)

### Building

```bash
# Development build (host architecture, Homebrew libusb)
make clean && make
make test

# Release build: universal .pkg inside a .dmg (no Homebrew needed)
make dist

# Redraw the app icon and the README screenshots
make icon
make screenshots
```

### Running

```bash
# Run the daemon (requires root)
sudo ./build/USBTether.app/Contents/MacOS/macostether --verbose

# Install locally
sudo make install
```

## Project Structure

```
src/              C source files (daemon)
include/          C header files
ui/               Swift menu bar application
resources/        App bundle resources and plists
build/            Build output (gitignored)
```

### Key Components

| File | Purpose |
|------|---------|
| `src/main.c` | Session orchestration, bridge threads, main loop |
| `src/proto_rndis.c` | RNDIS protocol driver (implements `proto_driver_t`) |
| `src/usb_device.c` | USB device discovery and bulk transfers |
| `src/utun.c` | macOS utun interface, routing, DNS |
| `src/dhcp.c` | DHCP client (raw packet construction) |
| `src/ipc.c` | Unix domain socket server for UI communication |
| `ui/*.swift` | Menu bar app: SwiftUI popover, IPC client, state model |
| `scripts/make-icon.swift` | Draws the app icon (`make icon`) |
| `scripts/render-screenshots.swift` | Renders the README screenshots (`make screenshots`) |

## Code Style

- **C code**: K&R style, 4-space indentation, no tabs
- **Naming**: `snake_case` for functions and variables, `UPPER_CASE` for constants
- **Headers**: Every `.c` file has a matching `.h` in `include/`
- **Logging**: Use `LOG_D()`, `LOG_I()`, `LOG_W()`, `LOG_E()` macros with a tag
- **Error handling**: Return -1 on error, 0 on success; log errors at the point they occur

## Adding a New Protocol Driver

The project uses a vtable-based protocol driver interface. To add support for a new USB tethering protocol (e.g., NCM or ECM):

1. **Create** `src/proto_ncm.c` and `include/proto_ncm.h` (if needed)
2. **Implement** the `proto_driver_t` interface (see `include/proto_driver.h`):
   - `init()` — Protocol initialization over USB
   - `get_mac()` — Get device MAC address
   - `wrap_frame()` — Wrap Ethernet frame for USB TX
   - `unwrap_data()` — Extract Ethernet frames from USB RX data
   - `keepalive()` — Periodic keepalive (optional)
   - `get_usb_filters()` — USB class/subclass/protocol for device discovery
   - `destroy()` — Cleanup
3. **Add** a `proto_ncm_create()` constructor
4. **Register** it in the driver lookup in `main.c`
5. **Update** the `Makefile` `SRCS` list

## Pull Request Process

1. Fork the repository and branch from `dev`.
2. Make your change. Keep each pull request to one topic.
3. Run `make clean && make && make test`. The build must have no new warnings.
4. Test with a real phone if your change touches USB, networking, or the menu bar app.
5. Open a pull request into `main` and fill in the template. CI must pass before merging.

## Reporting Issues

When reporting bugs, please include:

- macOS version (`sw_vers`)
- Hardware (Intel or Apple Silicon)
- Android device model
- Output of `sudo macostether --verbose` (redact any sensitive info)
- Steps to reproduce
