#!/bin/sh
# Runs Kiln's Linux integration scenario in the Ubuntu test VM (ADR 0024), from the host (Git
# Bash on Windows): copies this working tree in, builds it there, runs scenario.sh, prints its
# report. Exit code 0 = passed.
#   tests/vm/ubuntu/run.sh <vm-address> [user]
# Find the address with: Get-NetNeighbor -LinkLayerAddress <the VM's MAC> (see dev-setup.md).
set -eu
ip=${1:?usage: run.sh <vm-address> [user]}
user=${2:-sameed}
key=/e/VMs/kiln-ubuntu/ssh
cd "$(dirname "$0")/../../.."
git ls-files -co --exclude-standard | tar -cf - -T - |
    ssh -i "$key/id_ed25519" -o UserKnownHostsFile="$key/known_hosts" -o BatchMode=yes "$user@$ip" \
        'mkdir -p ~/kiln && cd ~/kiln && tar -xmf - && . ~/.cargo/env &&
         cargo build --workspace --locked 2>&1 | tail -1 && sh tests/vm/ubuntu/scenario.sh'
