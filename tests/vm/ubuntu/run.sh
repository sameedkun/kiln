#!/bin/sh
# Runs Kiln's Linux integration scenario in the Ubuntu test VM (ADR 0024), from the host (Git
# Bash on Windows): copies this working tree in, builds it there, runs scenario.sh, reboots the
# VM when it asks to (exit code 10, ADR 0034), runs after-reboot.sh once the desktop has logged
# in again, and prints the report. Exit code 0 = passed.
#   tests/vm/ubuntu/run.sh <vm-address> [user]
# Find the address with: Get-NetNeighbor -LinkLayerAddress <the VM's MAC> (see dev-setup.md).
set -eu
ip=${1:?usage: run.sh <vm-address> [user]}
user=${2:-sameed}
key=/e/VMs/kiln-ubuntu/ssh
vm() { ssh -i "$key/id_ed25519" -o UserKnownHostsFile="$key/known_hosts" -o BatchMode=yes \
    -o ConnectTimeout=10 "$user@$ip" "$@"; }
cd "$(dirname "$0")/../../.."
status=0
git ls-files -co --exclude-standard | tar -cf - -T - |
    vm 'mkdir -p ~/kiln && cd ~/kiln && tar -xmf - && . ~/.cargo/env &&
        cargo build --workspace --locked 2>&1 | tail -1 && sh tests/vm/ubuntu/scenario.sh' || status=$?
[ "$status" -eq 10 ] || exit "$status"

# A reboot, as a user's, with the desktop's automatic login; then a minute for kilnd's first
# watchdog pass (every 15 s) to bring the web server back.
echo "Rebooting the VM (ADR 0034)."
vm 'sudo systemctl reboot' 2>/dev/null || true
sleep 20
i=0
until vm true 2>/dev/null; do
    i=$((i + 1))
    [ $i -gt 30 ] && { echo "the VM didn't come back"; exit 1; }
    sleep 5
done
sleep 60
vm 'sh ~/kiln/tests/vm/ubuntu/after-reboot.sh'
