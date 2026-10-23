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
check "web install" "$K" web install
check "ca init" "$K" ca init
check "park the sites folder" "$K" park "$SITES"
check "secure php-app" "$K" secure php-app
check "hosts sync" "$K" hosts sync --yes
check "ca trust --system" "$K" ca trust --system --yes
check "shims install" "$K" shims install --yes
check "web start" "$K" web start

ports=$("$K" web status | grep -oE '127\.0\.0\.1:[0-9]+' | cut -d: -f2 | awk '!seen[$0]++')
http=$(echo "$ports" | sed -n 1p)
https=$(echo "$ports" | sed -n 2p)
echo "     ports: http $http, https $https"
check "a static site over HTTP, by its hosts name" \
    contains "static from kiln" curl -fsS "http://static-site.test:$http/"
check "a PHP site over HTTPS, trusted through the system store" \
    contains "via fpm-fcgi" curl -fsS "https://php-app.test:$https/"
check "an HTTP request to the secure site redirects to HTTPS" \
    contains "https://php-app.test" curl -sSI "http://php-app.test:$http/"
check "the php shim in a new terminal runs Kiln's PHP 8.4" \
    contains "8.4." sh -c "cd '$SITES/php-app' && bash -ic 'php -r \"echo PHP_VERSION;\"' 2>/dev/null"
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
