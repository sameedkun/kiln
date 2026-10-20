#!/usr/bin/env bash
# matrix.sh <nginx|caddy> — throughput matrix, then footprint. Leaves the server stopped.
set -euo pipefail
cd "$(dirname "$0")"
s="$1"; secs=15
for w in 1 4 8; do
  ./render.sh "$w"; ./workers.sh start "$w" 0; ./server.sh "$s" start
  for rep in 1 2; do ./bench.sh "$s-laravel-w$w-r$rep" blog.test http / http1 "$secs"; done
  if [ "$w" = 4 ]; then
    for rep in 1 2; do ./bench.sh "$s-laravel-tls-w4-r$rep" blog.test https / http2 "$secs"; done
    for rep in 1 2; do ./bench.sh "$s-static1mb-h1-r$rep" static.test https /asset-1mb.bin http1 "$secs"; done
    for rep in 1 2; do ./bench.sh "$s-static1mb-h2-r$rep" static.test https /asset-1mb.bin http2 "$secs"; done
    for rep in 1 2; do ./bench.sh "$s-static-small-h2-r$rep" static.test https /index.html http2 "$secs" 64; done
    powershell -NoProfile -Command "\$p = Get-CimInstance Win32_Process | Where-Object { \$_.ExecutablePath -like '*\\kiln\\spikes\\web-server\\.cache\\$s\\*' }; 'footprint-$s procs={0} working_set_MB={1:N1}' -f \$p.Count, ((\$p | Measure-Object WorkingSetSize -Sum).Sum / 1MB)" | tee -a .cache/footprint.txt
  fi
  ./server.sh "$s" stop; ./workers.sh stop
done
