#!/bin/sh
# Spike 03's control (docs/spikes/03-windows-cache.md): the same Laravel checks against Valkey,
# the real thing, in the Ubuntu test VM, so a failure on Garnet can be told apart from a
# mistake in the checks. Run from the host (Git Bash) after tests/vm/ubuntu/run.sh has built
# Kiln there. Ends with `kiln uninstall --delete-data` and removes its own folder.
#   spikes/windows-cache/run-ubuntu.sh <vm-address> [user]
set -eu
ip=${1:?usage: run-ubuntu.sh <vm-address> [user]}
user=${2:-sameed}
key=/e/VMs/kiln-ubuntu/ssh
vm() { ssh -i "$key/id_ed25519" -o UserKnownHostsFile="$key/known_hosts" -o BatchMode=yes "$user@$ip" "$@"; }
here=$(cd "$(dirname "$0")" && pwd)

vm 'rm -rf ~/kiln-spike && mkdir -p ~/kiln-spike'
scp -q -i "$key/id_ed25519" -o UserKnownHostsFile="$key/known_hosts" "$here/KilnSpike.php" "$user@$ip:kiln-spike/"
vm 'set -u; D=~/kiln-spike; K=~/kiln/target/debug/kiln; cd $D
    echo "== Valkey (control)"
    curl -fsSL -o valkey.tar.gz https://download.valkey.io/releases/valkey-9.0.7-noble-x86_64.tar.gz
    echo "08ed643e3c1b0f8cec8a85303bc097515376d576dd6f57bbbbd736a5103fe3d3  valkey.tar.gz" | sha256sum -c - || exit 1
    tar -xzf valkey.tar.gz
    server=$(find "$D" -name valkey-server -type f | head -1)
    "$server" --version
    "$server" --bind 127.0.0.1 --port 6379 --save "" --appendonly no --daemonize no > valkey.log 2>&1 &
    pid=$!
    sleep 1
    echo "== Laravel app with Kiln PHP and Composer"
    export COMPOSER_HOME=$D/composer-home COMPOSER_CACHE_DIR=$D/composer-cache
    $K php install 8.4 > php.txt 2>&1 && $K composer install > composer.txt 2>&1 && $K shims install --yes > shims.txt 2>&1
    composer=~/.local/share/kiln/bin/shims/composer
    php=~/.local/share/kiln/bin/shims/php
    $composer create-project laravel/laravel app --no-interaction --prefer-dist > create.txt 2>&1; echo "create-project exit: $?"
    cd app
    $composer require predis/predis --no-interaction > ../predis.txt 2>&1; echo "predis exit: $?"
    $php artisan --version
    mkdir -p app/Console/Commands && cp ../KilnSpike.php app/Console/Commands/
    sed -i "s/^REDIS_CLIENT=.*/REDIS_CLIENT=predis/; s/^CACHE_STORE=.*/CACHE_STORE=redis/" .env
    grep -q "^REDIS_CLIENT=" .env || echo "REDIS_CLIENT=predis" >> .env
    echo "== checks against Valkey"
    $php artisan kiln:spike; echo "exit: $?"
    kill $pid
    cd ~
    $K uninstall --yes --delete-data > /dev/null 2>&1; echo "kiln uninstalled: $?"
    rm -rf ~/kiln-spike'
