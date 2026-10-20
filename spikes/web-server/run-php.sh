#!/usr/bin/env bash
# Second pass for PHP-bound scenarios with stateless sessions (array driver), so session-file GC
# does not dominate. Appends to results.tsv with a "v2-" prefix.
set -uo pipefail
cd "$(dirname "$0")"
C=.cache
log() { echo "$(date +%T) $*" | tee -a $C/events.log; }
stop_all() { ./server.sh nginx stop; ./server.sh caddy stop; }
SECS=10

for w in 1 4 8; do
  ./workers.sh start "$w" 0; ./render.sh "$w"
  for round in 1 2 3; do
    for s in nginx caddy; do
      stop_all; ./server.sh "$s" start
      ./bench.sh "v2-$s-laravel-h1-w$w-r$round" blog.test http / http1 $SECS
      [ "$w" = 4 ] && ./bench.sh "v2-$s-laravel-tls-h2-w4-r$round" blog.test https / http2 $SECS
    done
  done
done
stop_all

# Recycling: PHP_FCGI_MAX_REQUESTS=50 so every worker recycles several times.
for s in nginx caddy; do
  ./workers.sh start 4 50; ./render.sh 4 0; stop_all; ./server.sh "$s" start
  ./bench.sh "v2-$s-recycle-50-under-load" blog.test http / http1 20
  log "$s recycle(50) restarts: $(wc -l < $C/run/restarts.log)"
done
stop_all

# Reload correctness: after the final reload the extra sites must be gone, i.e. extra1.test
# must no longer serve the static site's body.
./workers.sh start 4 0
static_body="$(cat $C/apps/static/index.html)"
for s in nginx caddy; do
  ./render.sh 4 20; stop_all; ./server.sh "$s" start
  b1="$(curl -s --resolve extra1.test:8080:127.0.0.1 http://extra1.test:8080/)"
  ./render.sh 4 0; ./server.sh "$s" reload; sleep 1
  b2="$(curl -s --resolve extra1.test:8080:127.0.0.1 http://extra1.test:8080/)"
  log "$s extra1 before-removal-serves-static=$([ "$b1" = "$static_body" ] && echo yes || echo no) after-removal-serves-static=$([ "$b2" = "$static_body" ] && echo yes || echo no) after-body-bytes=${#b2}"
done
stop_all
./workers.sh stop
log "v2 done"
