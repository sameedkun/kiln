#!/usr/bin/env bash
# bench.sh <label> <host> <scheme> <path> <http1|http2> <seconds> [connections]
# Appends one TSV row to .cache/results.tsv:
# label host proto rps p50_ms p99_ms ok non2xx errors
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
C="$HERE/.cache"
label="$1"; host="$2"; scheme="$3"; path="$4"; proto="$5"; secs="$6"; conns="${7:-32}"
port=8080; [ "$scheme" = https ] && port=8443
args=(-z "${secs}s" -c "$conns" --no-tui --output-format json
      --connect-to "$host:$port:127.0.0.1:$port")
[ "$scheme" = https ] && args+=(--cacert "$C/pki/root.crt")
[ "$proto" = http2 ] && args+=(--http2)
json="$("$C/dl/oha.exe" "${args[@]}" "$scheme://$host:$port$path")"
row="$(printf '%s' "$json" | "$C/php/php.exe" -r '
  $j = json_decode(stream_get_contents(STDIN), true);
  $s = $j["summary"]; $p = $j["latencyPercentiles"];
  $codes = $j["statusCodeDistribution"] ?? [];
  $ok = 0; $bad = 0; foreach ($codes as $c => $n) { if ($c >= 200 && $c < 300) $ok += $n; else $bad += $n; }
  // Requests still in flight when -z expires are reported as "aborted due to deadline"; they
  // are a property of the load generator, not server failures.
  $errs = 0;
  foreach ($j["errorDistribution"] ?? [] as $msg => $n) { if (stripos($msg, "deadline") === false) $errs += $n; }
  printf("%.0f\t%.2f\t%.2f\t%d\t%d\t%d", $s["requestsPerSec"], $p["p50"] * 1000, $p["p99"] * 1000, $ok, $bad, $errs);
')"
printf '%s\t%s\t%s\t%s\n' "$label" "$host$path" "$proto" "$row" | tee -a "$C/results.tsv"
