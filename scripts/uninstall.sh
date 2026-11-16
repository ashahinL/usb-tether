#!/bin/bash
set -euo pipefail
if [[ $EUID -ne 0 ]]; then
    exec sudo /bin/bash "$0" "$@"
fi
CONSOLE_USER=$(stat -f '%Su' /dev/console)
if [[ "$CONSOLE_USER" != root && "$CONSOLE_USER" != loginwindow && -n "$CONSOLE_USER" ]]; then
    pkill -u "$(id -u "$CONSOLE_USER")" -x USBTether 2>/dev/null || true
fi
launchctl bootout system/com.ashahin.macostether 2>/dev/null || true
for path in /Library/LaunchDaemons/com.ashahin.macostether.plist \
    "/Applications/USB Tether.app" /Applications/macOs_tether.app \
    /var/run/macostether.sock /var/log/macostether.log; do
    if [[ -e "$path" || -L "$path" ]]; then
        rm -rf "$path"
        printf 'Removed %s\n' "$path"
    fi
done
pkgutil --forget com.ashahin.macostether 2>/dev/null || true
echo 'USB Tether uninstalled.'
