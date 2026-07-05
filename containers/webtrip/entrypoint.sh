#!/bin/sh
# Picks the server mode from what's mounted at /certs:
#   both server.crt + server.key  -> https app on $HTTPS_PORT (default 8443),
#                                    $HTTP_PORT (default 8080) redirects to it
#   neither                       -> plain http app on $HTTP_PORT, no TLS needed
#                                    ($HTTPS_PORT is ignored)
#   only one                      -> almost certainly a broken mount; fail loudly
set -eu

CRT=/certs/server.crt
KEY=/certs/server.key
HTTP_PORT="${HTTP_PORT:-8080}"
HTTPS_PORT="${HTTPS_PORT:-8443}"

if [ -f "$CRT" ] && [ -f "$KEY" ]; then
  if [ "$HTTP_PORT" = "$HTTPS_PORT" ]; then
    echo "error: HTTP_PORT and HTTPS_PORT are both $HTTP_PORT; the redirect listener and the https server need distinct ports" >&2
    exit 1
  fi
  PORT="$HTTPS_PORT" exec node website/server.js \
    --cert "$CRT" --key "$KEY" --redirect-http "$HTTP_PORT"
elif [ -f "$CRT" ] || [ -f "$KEY" ]; then
  echo "error: only one of $CRT / $KEY is mounted; mount both for https or neither for http" >&2
  exit 1
fi

PORT="$HTTP_PORT" exec node website/server.js
