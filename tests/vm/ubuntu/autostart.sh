#!/bin/sh
# Checks Kiln's autostart across a real reboot in the Ubuntu test VM (P2-02, ADR 0026 §6): a
# site served, `kiln daemon enable`, reboot (the VM logs in to the desktop by itself), then
# kilnd must be running again and have brought the web server back. Run from the host (Git
# Bash) after run.sh has built Kiln in the VM. Ends with `kiln uninstall`. Exit 0 = passed.
#   tests/vm/ubuntu/autostart.sh <vm-address> [user]
set -u
ip=${1:?usage: autostart.sh <vm-address> [user]}
user=${2:-sameed}
key=/e/VMs/kiln-ubuntu/ssh
vm() { ssh -i "$key/id_ed25519" -o UserKnownHostsFile="$key/known_hosts" -o BatchMode=yes \
    -o ConnectTimeout=10 "$user@$ip" "$@"; }

vm 'set -e; K=~/kiln/target/debug/kiln; mkdir -p ~/kiln-autostart-site
    echo "autostart ok" > ~/kiln-autostart-site/index.html
    $K web install >/dev/null; $K link site --path ~/kiln-autostart-site >/dev/null
    $K web start | head -2; $K daemon enable --yes; $K daemon status; sync' || exit 1

echo "== reboot"
vm 'sudo systemctl reboot' 2>/dev/null
sleep 20
i=0
until vm true 2>/dev/null; do
    i=$((i + 1))
    [ $i -gt 30 ] && { echo "the VM didn't come back"; exit 1; }
    sleep 5
done
sleep 60   # the desktop's automatic login, then kilnd's first watchdog pass (every 15 s)

vm 'K=~/kiln/target/debug/kiln
    running() { ps -u "$(id -u)" -o comm= | grep -qx "$1"; }
    port=$($K web status | grep -oE "127\.0\.0\.1:[0-9]+" | head -1 | cut -d: -f2)
    ok=0
    running kilnd && echo "kilnd running after reboot: yes" || { echo "kilnd running after reboot: no"; ok=1; }
    running kiln-caddy && echo "web server running after reboot: yes" || { echo "web server running after reboot: no"; ok=1; }
    curl -fsS -H "Host: site.test" "http://127.0.0.1:${port:-8080}/" | grep -q "autostart ok" \
        && echo "site answers after reboot: yes" || { echo "site answers after reboot: no"; ok=1; }
    echo "kilnd.log (end):"; tail -6 ~/.local/share/kiln/logs/kilnd.log
    $K uninstall --yes >/dev/null && echo "uninstalled" || ok=1
    rm -rf ~/kiln-autostart-site
    [ $ok -eq 0 ] && echo "== PASSED" || echo "== FAILED"
    exit $ok'
