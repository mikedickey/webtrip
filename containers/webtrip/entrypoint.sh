#!/bin/sh
# Picks the server mode from what's mounted at /certs:
#   both server.crt + server.key  -> https on $PORT (default 443), port 80 redirects
#   neither                       -> plain http on $PORT (default 80), no TLS needed
#   only one                      -> almost certainly a broken mount; fail loudly
set -eu

CRT=/certs/server.crt
KEY=/certs/server.key

if [ -f "$CRT" ] && [ -f "$KEY" ]; then
  export PORT="${PORT:-443}"
  exec node website/server.js --cert "$CRT" --key "$KEY" --redirect-http 80
elif [ -f "$CRT" ] || [ -f "$KEY" ]; then
  echo "error: only one of $CRT / $KEY is mounted; mount both for https or neither for http" >&2
  exit 1
fi

export PORT="${PORT:-80}"
exec node website/server.js
