# Helpers shared by scenario.sh and after-reboot.sh (ADR 0024, ADR 0034), sourced after they
# set K (the kiln program). The work folder is in the home folder, not /tmp: it has to outlast
# the reboot between the two parts.
HERE=$(cd "$(dirname "$0")" && pwd)
WORK="$HOME/kiln-test"
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

compare_snapshot() { # compare_snapshot <file> <what>: against the first snapshot
    if diff -u "$WORK/before.txt" "$WORK/$1" > "$WORK/$1.diff"; then
        echo "PASS the machine is as it was $2: $(wc -l < "$WORK/before.txt") facts compared"
    else
        echo "FAIL the machine differs $2:"
        sed 's/^/     /' "$WORK/$1.diff"
        failed=1
    fi
}
