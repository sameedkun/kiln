#!/bin/sh
# The second part of Kiln's Linux integration scenario (ADR 0034): after scenario.sh left kilnd
# enabled and running, run.sh rebooted the VM and the test user logged in to the desktop by
# itself. Checks that the login started kilnd and that it brought the web server back, then
# uninstalls, snapshots again and compares with scenario.sh's first snapshot. Then the
# .internal stage (ADR 0033). Exit 0 = the whole scenario passed.
#   tests/vm/ubuntu/after-reboot.sh [path/to/kiln]
set -u
K=${1:-$HOME/kiln/target/debug/kiln}
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"
failed=$(cat "$WORK/failed")
read -r http https < "$WORK/ports"

echo "== after the reboot (ADR 0034)"
running() { ps -u "$(id -u)" -o comm= | grep -qx "$1"; }
# Nothing has run a kiln command since the boot: what runs was started by the login, which
# can take a while on a slow boot.
started_at_login() {
    for i in $(seq 60); do
        running kilnd && return 0
        sleep 3
    done
    echo "kilnd is not running 3 minutes after the boot"
    return 1
}
check "kilnd was started at login by its autostart entry" started_at_login
came_back() { # within a minute: kilnd's watchdog passes every 15 s
    for i in $(seq 20); do
        running kiln-caddy && running kiln-supervisor && return 0
        sleep 3
    done
    echo "kiln-caddy or kiln-supervisor is not running"
    return 1
}
check "kilnd brought the web server and the PHP pools back" came_back
daemon_reports() { # once the watchdog's `kiln web start` has finished
    for i in $(seq 20); do
        "$K" daemon status > "$WORK/status.txt" 2>&1
        grep -q 'Web server: running' "$WORK/status.txt" && grep -q 'Starts at login: yes' "$WORK/status.txt" && return 0
        sleep 3
    done
    cat "$WORK/status.txt"
    return 1
}
check "daemon status: the web server runs, and kilnd starts at login" daemon_reports
check "a static site over HTTP, by its hosts name" \
    contains "static from kiln" curl -fsS "http://static-site.test:$http/"
check "a PHP site over HTTPS, trusted through the system store, on its own PHP 8.3" \
    sh -c "curl -fsS 'https://php-app.test:$https/' | grep -q 'php 8.3.* via fpm-fcgi'"
check "the Laravel app over HTTPS" contains "Laravel" curl -fsS "https://laravel-app.test:$https/"

echo "== uninstall"
refused() { # Mailpit's caught mail is service data (ADR 0036 §9)
    if "$K" uninstall --yes > "$WORK/refused.txt" 2>&1; then
        echo "uninstall went ahead without --delete-data"
        return 1
    fi
    grep -q "hold data" "$WORK/refused.txt" && [ -d "$HOME/.local/share/kiln" ] && [ -d "$HOME/.local/state/kiln" ]
}
check "kiln uninstall refuses while services hold data, and changes nothing" refused
check "kiln uninstall --delete-data" "$K" uninstall --yes --delete-data
sleep 1
sh "$HERE/snapshot.sh" > "$WORK/after.txt"

echo "== compare (ADR 0024 §2)"
compare_snapshot after.txt "after uninstall"

echo "== another TLD from the start (P2-10, ADR 0033)"
# A fresh Kiln that chose .internal: sites, the hosts block and the root follow it. Checked
# over HTTPS against the new root itself, without trusting it anywhere.
check "web install, then ca init --tld internal: a root for .internal" \
    sh -c "'$K' web install > /dev/null && '$K' ca init --tld internal | grep -q 'names under .internal'"
check "link static-site --secure: linked as static-site.internal" \
    contains "static-site.internal" "$K" link static-site --path "$SITES/static-site" --secure
check "hosts sync lists static-site.internal" \
    sh -c "'$K' hosts sync --yes > /dev/null && grep -q 'static-site.internal' /etc/hosts"
internal_https() {
    "$K" web start > "$WORK/internal-start.txt" 2>&1 || return 1
    url=$(grep -oE 'https://static-site\.internal[^ ]*' "$WORK/internal-start.txt" | head -1)
    root="$("$K" ca info | sed -n 's/^CA directory: //p')/root.crt"
    [ -n "$url" ] && curl -fsS --cacert "$root" "$url" | grep -q 'static from kiln'
}
check "static-site.internal over HTTPS, signed by the .internal root" internal_https
check "kiln uninstall again" "$K" uninstall --yes
sleep 2
sh "$HERE/snapshot.sh" > "$WORK/after2.txt"
compare_snapshot after2.txt "after the second uninstall too"

if [ "$failed" -eq 0 ]; then
    echo "== PASSED"
    rm -rf "$WORK"
else
    echo "== FAILED (kept $WORK)"
fi
exit "$failed"
