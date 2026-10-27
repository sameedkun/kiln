#!/bin/sh
# Prints everything Kiln may touch on this machine, one fact per line, for comparing before
# and after a Kiln session (ADR 0024 §2). Plain files by SHA-256; databases by listing.
set -u

file() {
    if [ -e "$1" ]; then
        echo "file $1 $(sha256sum < "$1" | cut -d' ' -f1) $(stat -c %a "$1")"
    else
        echo "file $1 absent"
    fi
}

for f in /etc/hosts "$HOME/.profile" "$HOME/.bashrc" "$HOME/.zshrc" \
    "$HOME/.config/fish/conf.d/kiln.fish" /etc/ssl/certs/ca-certificates.crt; do
    file "$f"
done
echo "anchors: $(ls /usr/local/share/ca-certificates 2>/dev/null | tr '\n' ' ')"

# NSS databases (Chrome's and each Firefox profile's), by their certificate listings.
for db in "$HOME/.pki/nssdb" "$HOME"/snap/firefox/common/.mozilla/firefox/*/ "$HOME"/.mozilla/firefox/*/; do
    db=${db%/}
    [ -f "$db/cert9.db" ] || continue
    certutil -d "sql:$db" -L 2>/dev/null | tail -n +4 | sed "s|^|nss $db: |"
done

# Kiln's items in the desktop keyring (unlocked, then locked lists).
echo "keyring: $(busctl --user call org.freedesktop.secrets /org/freedesktop/secrets \
    org.freedesktop.Secret.Service SearchItems 'a{ss}' 1 application kiln 2>&1)"

# Login items (P2-02): the autostart folder and what is in it.
autostart="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
if [ -d "$autostart" ]; then
    echo "autostart: $(ls -A "$autostart" | tr '\n' ' ')"
else
    echo "autostart: no folder"
fi

for d in "$HOME/.local/share/kiln" "$HOME/.local/state/kiln"; do
    if [ -e "$d" ]; then echo "dir $d present"; else echo "dir $d absent"; fi
done

# Kiln's processes, and anything of this user's listening on a port.
echo "processes: $(ps -u "$(id -u)" -o comm= | grep -E '^(kiln-caddy|kiln-supervisor|kilnd|php-fpm)$' | sort | uniq -c | tr -s ' ' | tr '\n' ';')"
echo "listening: $(ss -ltnpH 2>/dev/null | grep 'users:' | awk '{print $4}' | sort | tr '\n' ' ')"
