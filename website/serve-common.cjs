// Shared between website/server.js (production) and the Vite dev middleware
// (vite.config.ts): mapping for the demo's WASM artifacts and the headers the
// demo needs. CommonJS so both can load it.
const path = require('path');

const REPO_ROOT = path.join(__dirname, '..');

const MIME_TYPES = {
  '.html': 'text/html',
  '.js': 'application/javascript',
  '.wasm': 'application/wasm',
  '.json': 'application/json',
  '.css': 'text/css',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.gif': 'image/gif',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon'
};

// COOP/COEP make the page cross-origin isolated, which SharedArrayBuffer
// (and therefore the demo's threaded WASM) requires.
const CROSS_ORIGIN_ISOLATION_HEADERS = {
  'Cross-Origin-Embedder-Policy': 'require-corp',
  'Cross-Origin-Opener-Policy': 'same-origin'
};

// The demo route loads the wasm-pack output at runtime from `{origin}/pkg/...`
// (see src/audio/webtransport.rs::wasm_module_url and website/src/lib/webtrip.ts),
// so /pkg/* is served from the repo root. Returns null for anything else —
// those are website files.
function resolvePkgFile(urlPath) {
  return urlPath.startsWith('/pkg/') ? path.join(REPO_ROOT, urlPath) : null;
}

// SPA routes are canonical without a trailing slash: the WASM worker URL is
// resolved against the page path's directory, so serving the demo at `/demo/`
// would make it look for `/demo/pkg/webtrip.js`. Returns the redirect target,
// or null if the path is already canonical.
function trailingSlashRedirectTarget(urlPath) {
  return urlPath !== '/' && urlPath.endsWith('/') ? urlPath.slice(0, -1) : null;
}

module.exports = {
  MIME_TYPES,
  CROSS_ORIGIN_ISOLATION_HEADERS,
  resolvePkgFile,
  trailingSlashRedirectTarget
};
