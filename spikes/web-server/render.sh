#!/usr/bin/env bash
# Renders nginx.conf and Caddyfile for the spike.
#   render.sh <workers> [extra_sites]
# extra_sites adds N dummy static sites (used by the reload test).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
C="$(cd "$HERE/.cache" && pwd -W)"   # Windows path with forward slashes, e.g. D:/Code/...
workers="$1"; extra="${2:-0}"

upstreams=""; caddy_upstreams=""
for i in $(seq 1 "$workers"); do
  upstreams+="        server 127.0.0.1:$((9100 + i)) max_fails=0;"$'\n'
  caddy_upstreams+=" 127.0.0.1:$((9100 + i))"
done

# ---------- nginx ----------
mkdir -p "$HERE/.cache/nginx/conf/sites"
cat > "$HERE/.cache/nginx/conf/spike.conf" <<EOF
worker_processes 1;
error_log logs/error.log warn;
pid logs/nginx.pid;
events { worker_connections 1024; }
http {
    include mime.types;
    default_type application/octet-stream;
    access_log off;
    sendfile on;
    keepalive_timeout 65;

    upstream php_pool {
$upstreams    }

    server {
        listen 127.0.0.1:8080;
        listen 127.0.0.1:8443 ssl;
        http2 on;
        server_name blog.test;
        ssl_certificate     "$C/pki/blog.test.crt";
        ssl_certificate_key "$C/pki/blog.test.key";
        root "$C/apps/laravel/public";
        index index.php;
        location / { try_files \$uri \$uri/ /index.php?\$query_string; }
        location ~ \.php\$ {
            fastcgi_pass php_pool;
            fastcgi_next_upstream error timeout;
            include fastcgi_params;
            fastcgi_param SCRIPT_FILENAME \$document_root\$fastcgi_script_name;
        }
    }

    server {
        listen 127.0.0.1:8443 ssl;
        http2 on;
        server_name static.test;
        ssl_certificate     "$C/pki/static.test.crt";
        ssl_certificate_key "$C/pki/static.test.key";
        root "$C/apps/static";
    }

    server {
        listen 127.0.0.1:8443 ssl;
        server_name example.com;
        ssl_certificate     "$C/pki/example.com.crt";
        ssl_certificate_key "$C/pki/example.com.key";
        return 200 "should never be trusted\n";
    }

    include sites/*.conf;
}
EOF
rm -f "$HERE/.cache/nginx/conf/sites/"*.conf
for i in $(seq 1 "$extra"); do
  cat > "$HERE/.cache/nginx/conf/sites/extra$i.conf" <<EOF
server { listen 127.0.0.1:8080; server_name extra$i.test; root "$C/apps/static"; }
EOF
done

# ---------- Caddy ----------
{
cat <<EOF
{
    admin localhost:2019
    persist_config off
    default_bind 127.0.0.1
    http_port 8080
    https_port 8443
    auto_https disable_redirects
    skip_install_trust
    storage file_system "$C/caddy-data"
    pki {
        ca kiln {
            name "Kiln Spike CA"
            root {
                cert "$C/pki/root.crt"
                key "$C/pki/root.key"
            }
        }
    }
    log {
        level WARN
    }
}

(laravel) {
    root * "$C/apps/laravel/public"
    php_fastcgi$caddy_upstreams {
        lb_policy round_robin
        lb_try_duration 2s
        lb_try_interval 50ms
        lb_retry_match {
            method GET HEAD
        }
    }
    file_server
}

blog.test:8443 {
    tls "$C/pki/blog.test.crt" "$C/pki/blog.test.key"
    import laravel
}

http://blog.test:8080 {
    import laravel
}

static.test:8443 {
    tls "$C/pki/static.test.crt" "$C/pki/static.test.key"
    root * "$C/apps/static"
    file_server
}

# Leaf issued on demand by Caddy's PKI app from *our* name-constrained root.
caddyca.test:8443 {
    tls {
        issuer internal {
            ca kiln
        }
    }
    respond "issued by caddy pki" 200
}

example.com:8443 {
    tls "$C/pki/example.com.crt" "$C/pki/example.com.key"
    respond "should never be trusted" 200
}
EOF
for i in $(seq 1 "$extra"); do
  printf '\nhttp://extra%s.test:8080 {\n    root * "%s/apps/static"\n    file_server\n}\n' "$i" "$C"
done
} > "$HERE/.cache/Caddyfile"
