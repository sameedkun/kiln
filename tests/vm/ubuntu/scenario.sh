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
    sh -c "cd '$SITES' && bash -ic 'composer create-project laravel/laravel laravel-app --no-interaction --prefer-dist' 2>&1"
check "WordPress, from wordpress.org" \
    sh -c "curl -fsSL https://wordpress.org/latest.tar.gz | tar -xz -C '$SITES' && mv '$SITES/wordpress' '$SITES/wp-site'"

check "ca init" "$K" ca init
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
    contains "WordPress" curl -fsSL "https://wp-site.test:$https/"
check "an HTTP request to the secure site redirects to HTTPS" \
    contains "https://php-app.test" curl -sSI "http://php-app.test:$http/"
shim_php() { sh -c "cd '$1' && bash -ic 'php -r \"echo PHP_VERSION;\"' 2>/dev/null"; }
check "the php shim in a new terminal runs PHP 8.3 in php-app" contains "8.3." shim_php "$SITES/php-app"
check "the php shim in a new terminal runs PHP 8.4 in laravel-app" contains "8.4." shim_php "$SITES/laravel-app"
check "kiln doctor reports no problems" "$K" doctor
check "kiln history lists the system changes" contains "in effect" "$K" history

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
