#!/bin/sh
# Test-VM-only settings for Kiln's integration scenario (ADR 0024 §3). Run once in the Ubuntu
# test VM, never on a real machine: it lets the test user run pkexec without a password, so
# Kiln's polkit prompts answer themselves. (The user already has password-less sudo there.)
set -eu
user=$(id -un)
rule=/etc/polkit-1/rules.d/49-kiln-test.rules
sudo tee "$rule" > /dev/null <<EOF
// Kiln test VM only (ADR 0024 §3): $user may run pkexec without a password.
polkit.addRule(function (action, subject) {
    if (action.id == "org.freedesktop.policykit.exec" && subject.user == "$user") {
        return polkit.Result.YES;
    }
});
EOF
sudo apt-get install -y -qq libnss3-tools curl > /dev/null
echo "Wrote $rule; certutil and curl installed."
