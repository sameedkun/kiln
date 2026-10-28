#!/bin/sh
# Kiln's Linux integration scenario (ADR 0024 §1): snapshot, a full Kiln session, uninstall,
# snapshot again, compare. Run in the Ubuntu test VM as the test user:
#   tests/vm/ubuntu/scenario.sh [path/to/kiln]
# Needs the test VM's polkit rule (setup-test-vm.sh), so pkexec doesn't ask. Exit 0 = pass.
set -u
K=${1:-$HOME/kiln/target/debug/kiln}
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
SITES="$WORK/Sites"
failed=0

check() { # check <description> <command> [args…]: runs it and reports PASS or FAIL
    desc=$1
    shift
    if out=$("$@" 2>&1); then
        echo "PASS $desc"
    else
        echo "FAIL $desc"
        printf '%s\n' "$out" | tail -8 | sed 's/^/     /'
        failed=1
    fi
}

contains() { # contains <text> <command> [args…]: the command's output contains text
    text=$1
    shift
    "$@" 2>&1 | grep -qF -- "$text"
}

sh "$HERE/snapshot.sh" > "$WORK/before.txt"
if grep -q "dir .* present" "$WORK/before.txt"; then
    echo "FAIL Kiln is already set up here; run \`$K uninstall\` first"
    exit 2
fi

echo "== session"
mkdir -p "$SITES/static-site" "$SITES/php-app"
echo '<h1>static from kiln</h1>' > "$SITES/static-site/index.html"
echo '<?php echo "php ", PHP_VERSION, " via ", PHP_SAPI;' > "$SITES/php-app/index.php"
# Project config (P2-04, ADR 0027): a parked project whose kiln.toml asks for PHP 8.3 and an
# alias, and a project elsewhere with a Laravel Herd herd.yml to import.
mkdir -p "$SITES/toml-app" "$WORK/herd-app"
echo '<?php echo "toml ", PHP_VERSION;' > "$SITES/toml-app/index.php"
printf '[site]\naliases = ["toml-alias"]\n[php]\nversion = "8.3"\n[node]\nversion = "20"\n' > "$SITES/toml-app/kiln.toml"
# Node (P2-05, ADR 0028): php-app asks for an LTS line by name in .nvmrc.
echo lts/iron > "$SITES/php-app/.nvmrc"
echo '<?php echo "herd-import ", PHP_VERSION;' > "$WORK/herd-app/index.php"
printf "name: herd-demo\nphp: '8.3'\nsecured: true\naliases:\n  - herd-alias\n" > "$WORK/herd-app/herd.yml"
check "php install 8.4" "$K" php install 8.4
check "php install 8.3 (a second version, for per-site PHP)" "$K" php install 8.3
check "composer install" "$K" composer install
check "shims install" "$K" shims install --yes
check "web install" "$K" web install

# Real apps (plan.md, Phase 1 exit criteria): a new Laravel project made with Kiln's composer
# shim in a new terminal, and WordPress from wordpress.org. Composer's cache and config stay
# in $WORK, so the run leaves nothing of Composer's own behind either.
export COMPOSER_HOME="$WORK/composer-home" COMPOSER_CACHE_DIR="$WORK/composer-cache"
check "a new Laravel project, made with Kiln's composer shim" \
    sh -c "cd '$SITES' && for attempt in 1 2; do rm -rf laravel-app; bash -ic 'composer create-project laravel/laravel laravel-app --no-interaction --prefer-dist' 2>&1 && exit 0; done; exit 1"
check "WordPress, from wordpress.org" \
    sh -c "curl -fsSL --retry 5 --retry-delay 5 --retry-all-errors https://wordpress.org/latest.tar.gz | tar -xz -C '$SITES' && mv '$SITES/wordpress' '$SITES/wp-site'"

check "ca init" "$K" ca init
check "kiln import herd: herd.yml becomes kiln.toml and the folder is linked (P2-04)" \
    sh -c "'$K' import herd '$WORK/herd-app' > '$WORK/import.txt' && grep -q herd-demo '$WORK/import.txt' && grep -q 'version = \"8.3\"' '$WORK/herd-app/kiln.toml'"
check "park the sites folder" "$K" park "$SITES"
check "secure php-app" "$K" secure php-app
check "secure laravel-app" "$K" secure laravel-app
check "secure wp-site" "$K" secure wp-site
check "isolate php-app to PHP 8.3" "$K" isolate 8.3 php-app
check "hosts sync" "$K" hosts sync --yes
check "ca trust --system" "$K" ca trust --system --yes
check "web start" "$K" web start

ports=$("$K" web status | grep -oE '127\.0\.0\.1:[0-9]+' | cut -d: -f2 | awk '!seen[$0]++')
http=$(echo "$ports" | sed -n 1p)
https=$(echo "$ports" | sed -n 2p)
echo "     ports: http $http, https $https"
check "a static site over HTTP, by its hosts name" \
    contains "static from kiln" curl -fsS "http://static-site.test:$http/"
check "a PHP site over HTTPS, trusted through the system store, on its own PHP 8.3" \
    sh -c "curl -fsS 'https://php-app.test:$https/' | grep -q 'php 8\.3\..* via fpm-fcgi'"
check "the Laravel app over HTTPS (served from public/, PHP 8.4)" \
    contains "Laravel" curl -fsS "https://laravel-app.test:$https/"
check "WordPress over HTTPS (its setup page)" \
    contains "Setup Configuration File" curl -fsSL "https://wp-site.test:$https/wp-admin/setup-config.php?step=1"
check "kiln.toml: PHP 8.3 for toml-app (P2-04)" \
    contains "toml 8.3." curl -fsS "http://toml-app.test:$http/"
check "kiln.toml: and for its alias" contains "toml 8.3." curl -fsS "http://toml-alias.test:$http/"
check "the project imported from herd.yml: HTTPS and PHP 8.3" \
    contains "herd-import 8.3." curl -fsS "https://herd-demo.test:$https/"
check "the imported project's alias" contains "herd-import 8.3." curl -fsS "https://herd-alias.test:$https/"
check "kiln which names kiln.toml as the source" \
    contains "kiln.toml" sh -c "cd '$SITES/toml-app' && '$K' which"
check "an HTTP request to the secure site redirects to HTTPS" \
    contains "https://php-app.test" curl -sSI "http://php-app.test:$http/"
shim_php() { sh -c "cd '$1' && bash -ic 'php -r \"echo PHP_VERSION;\"' 2>/dev/null"; }
check "the php shim in a new terminal runs PHP 8.3 in php-app" contains "8.3." shim_php "$SITES/php-app"
check "the php shim follows kiln.toml in toml-app" contains "8.3." shim_php "$SITES/toml-app"
check "the php shim in a new terminal runs PHP 8.4 in laravel-app" contains "8.4." shim_php "$SITES/laravel-app"

echo "== Node (P2-05, ADR 0028)"
# npm's cache stays in $WORK, so the run leaves nothing of npm's own behind either.
export npm_config_cache="$WORK/npm-cache"
# The whole output first: `grep -q` stops reading at its match, and Kiln stops (quietly) when
# its output is closed, mid-install.
check "node install 22 and lts/iron (signature-checked SHASUMS)" \
    sh -c "'$K' node install 22 > '$WORK/node.txt' && '$K' node install lts/iron >> '$WORK/node.txt' && grep -q 'Checked SHASUMS256.txt' '$WORK/node.txt' && grep -q 'Installed Node 20\.' '$WORK/node.txt'"
shim_node() { sh -c "cd '$1' && bash -ic 'node -v' 2>/dev/null"; }
check "the node shim in a new terminal follows kiln.toml (20) in toml-app" contains "v20." shim_node "$SITES/toml-app"
check "the node shim follows .nvmrc (lts/iron) in php-app" contains "v20." shim_node "$SITES/php-app"
check "the node shim uses the default (22) in laravel-app" contains "v22." shim_node "$SITES/laravel-app"
check "kiln which names the .nvmrc" contains ".nvmrc asks for lts/iron" sh -c "cd '$SITES/php-app' && '$K' which"
# One retry for npm install: a dropped download is the network, not Kiln.
check "npm install and npm run build in the Laravel app, in a new terminal, with Kiln's Node" \
    sh -c "cd '$SITES/laravel-app' && for attempt in 1 2; do bash -ic 'npm install --no-audit --no-fund' && break; done && bash -ic 'npm run build' && test -f public/build/manifest.json"
check "kiln doctor reports no problems" "$K" doctor
check "kiln history lists the system changes" contains "in effect" "$K" history

echo "== kilnd (ADR 0026)"
caddy_pid() { ps -u "$(id -u)" -o pid=,comm= | awk '$2 == "kiln-caddy" {print $1}'; }
restarted() { # restarted <old pid>: waits up to a minute for a new kiln-caddy
    i=0
    while [ $i -lt 20 ]; do
        sleep 3
        now=$(caddy_pid)
        [ -n "$now" ] && [ "$now" != "$1" ] && return 0
        i=$((i + 1))
    done
    return 1
}
check "daemon start" "$K" daemon start
check "daemon status reports the web server running" contains "Web server: running" "$K" daemon status
old=$(caddy_pid)
kill -9 "$old"
check "the daemon restarts a web server that died" restarted "$old"
check "Kiln's sites answer after that restart" \
    sh -c "for i in 1 2 3 4 5 6 7 8 9 10; do curl -fsS 'http://static-site.test:$http/' | grep -q 'static from kiln' && exit 0; sleep 2; done; exit 1"
check "daemon status counts the restart" contains "Restarts by the daemon: 1" "$K" daemon status
check "daemon call web.status (the API)" contains '"running"' "$K" daemon call web.status
intermediate_sha() { "$K" ca info | grep 'SHA-256' | sed -n 2p; }
check "daemon status shows the intermediate certificate's days left (P2-03)" \
    contains "Intermediate certificate:" "$K" daemon status
renewed() { # a new intermediate, a restarted server, and HTTPS still trusted
    before=$(intermediate_sha)
    old=$(caddy_pid)
    "$K" daemon call ca.renew '{"force":true}' | grep -q '"ok": true' || return 1
    [ "$(intermediate_sha)" != "$before" ] || { echo "the intermediate didn't change"; return 1; }
    [ "$(caddy_pid)" != "$old" ] || { echo "the web server didn't restart"; return 1; }
    # Caddy issues the sites' certificates in the background right after the restart.
    for i in $(seq 20); do
        curl -fsS "https://php-app.test:$https/" 2>/dev/null | grep -q 'via fpm-fcgi' && return 0
        sleep 1
    done
    return 1
}
check "renewal through the daemon: a new intermediate, the server restarts to use it, HTTPS keeps working (P2-03)" renewed
check "daemon status counts the renewal" contains "Renewals by the daemon: 1" "$K" daemon status
check "daemon stop leaves the web server running" \
    sh -c "'$K' daemon stop && curl -fsS 'http://static-site.test:$http/' | grep -q 'static from kiln'"
entry="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/kiln-kilnd.desktop"
check "daemon enable: an XDG autostart entry starts kilnd at login (P2-02)" \
    sh -c "'$K' daemon enable --yes && grep -q '^Exec=.*kilnd' '$entry' && '$K' daemon status | grep -q 'Starts at login: yes'"
check "daemon disable removes the autostart entry" \
    sh -c "'$K' daemon disable --yes && [ ! -e '$entry' ]"
# On again, and running, so uninstall has to stop it and remove the entry.
check "daemon enable (again, for uninstall)" "$K" daemon enable --yes

echo "== uninstall"
check "kiln uninstall" "$K" uninstall --yes
sleep 1
sh "$HERE/snapshot.sh" > "$WORK/after.txt"

echo "== compare (ADR 0024 §2)"
if diff -u "$WORK/before.txt" "$WORK/after.txt" > "$WORK/diff.txt"; then
    echo "PASS the machine is as it was: $(wc -l < "$WORK/before.txt") facts compared"
else
    echo "FAIL the machine differs after uninstall:"
    sed 's/^/     /' "$WORK/diff.txt"
    failed=1
fi

if [ "$failed" -eq 0 ]; then
    echo "== PASSED"
    rm -rf "$WORK"
else
    echo "== FAILED (kept $WORK)"
fi
exit "$failed"
