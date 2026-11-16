# C daemon security fixes

Status: tasks 1 to 3 are done. The live check on the phone after install has not been run.

The daemon runs as root. These tasks fix the findings from the security review. The Rust rewrite is parked on the `rust-rewrite` branch.

## Tasks

1. Done. Bounds-check every length the phone sends. The RNDIS message walk checks `offset + msg_len` in 32 bits, so a large `msg_len` wraps and passes. The DHCP reply parser trusts `ip_hl` and reads option 53 and option 54 past the end. Use one RNDIS walk for the bridge and DHCP, and one DHCP reply parser for the offer and the ACK. Add `make test`, built with AddressSanitizer.
2. Done. Stop writing root-owned files in `/tmp`. Nothing reads the stats JSON, nothing creates the stop file, and nothing reads the DNS backup, so remove all three. Move the LaunchDaemon log to `/var/log/macostether.log`.
3. Done. Lock down the control socket. Move it from `/tmp/macostether.sock` to `/var/run/macostether.sock`. `/tmp` lets another user swap the path before the daemon runs `chmod` on it as root. Accept only root and the user who owns `/dev/console`, checked with `getpeereid`. Update the menu-bar app path.
4. Not done: a VID/PID allowlist. A malicious USB device can report any VID and PID, so an allowlist does not stop it. After task 1, a hostile device gets bounds-checked parsing. An allowlist would also break every phone not on the list.

## Checks

- `make` builds with no new warnings.
- `make test` passes under AddressSanitizer and UndefinedBehaviorSanitizer.
- After install, the menu-bar app connects to the new socket and the phone tethers.
