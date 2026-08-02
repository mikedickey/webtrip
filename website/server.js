// Static server for webtrip.dev: serves the built React site (website/dist)
// and the demo's WASM artifacts (/pkg) from the repo root. Sets the COOP/COEP
// headers required for SharedArrayBuffer.
import http from 'node:http';
import https from 'node:https';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  MIME_TYPES,
  CROSS_ORIGIN_ISOLATION_HEADERS,
  resolvePkgFile,
  safeUrlPath,
  trailingSlashRedirectTarget
} from './serve-common.cjs';

const SITE_DIST = path.join(path.dirname(fileURLToPath(import.meta.url)), 'dist');

// Parse --key, --cert, and --redirect-http arguments
const args = process.argv.slice(2);
let keyFile, certFile, redirectPort;
for (let i = 0; i < args.length; i++) {
  if (args[i] === '--key' && args[i + 1]) keyFile = args[++i];
  else if (args[i] === '--cert' && args[i + 1]) certFile = args[++i];
  else if (args[i] === '--redirect-http' && args[i + 1]) redirectPort = parseInt(args[++i], 10);
}

const useTLS = !!(keyFile && certFile);
const PORT = process.env.PORT ? parseInt(process.env.PORT, 10) : (useTLS ? 8443 : 3000);

// headersSent is checked because a failure can also surface from the async
// readFile callback, by which point a response may already be in flight.
const sendServerError = (res, detail) => {
  if (!res.headersSent) res.writeHead(500);
  res.end(detail ? `Server Error: ${detail}` : 'Server Error', 'utf-8');
};

// A synchronous throw out of the request handler is an uncaught exception that
// kills the process, so no single malformed request may escape this wrapper.
const handler = (req, res) => {
  try {
    serve(req, res);
  } catch (err) {
    // req.url is attacker-controlled: drop the query string so its contents
    // never reach the log, and JSON-escape what remains.
    const loggedPath = JSON.stringify((req.url ?? '').split('?')[0]);
    // A `null`/`undefined` or non-Error throw would make `err.message` throw a
    // second time — out of the catch, past this wrapper, and into the uncaught
    // handler the wrapper exists to prevent. Pass the value to console.error
    // rather than stringifying it, so formatting can't throw either.
    console.error(`Request failed for ${loggedPath}:`, err?.message ?? err);
    sendServerError(res);
  }
};

const serve = (req, res) => {
  const urlPath = safeUrlPath(req.url);
  if (!urlPath) {
    res.writeHead(400);
    res.end('Bad Request', 'utf-8');
    return;
  }

  const redirect = trailingSlashRedirectTarget(urlPath);
  if (redirect) {
    res.writeHead(301, { Location: redirect });
    res.end();
    return;
  }

  let filePath = resolvePkgFile(urlPath) || path.join(SITE_DIST, urlPath === '/' ? 'index.html' : urlPath);
  // SPA fallback: extensionless paths (e.g. /docs) are client-side routes.
  if (!path.extname(filePath) && !fs.existsSync(filePath)) {
    filePath = path.join(SITE_DIST, 'index.html');
  }

  const extname = String(path.extname(filePath)).toLowerCase();
  const contentType = MIME_TYPES[extname] || 'application/octet-stream';

  fs.readFile(filePath, (error, content) => {
    if (error) {
      if (error.code === 'ENOENT') {
        res.writeHead(404, { 'Content-Type': 'text/html' });
        res.end('<h1>404 Not Found</h1>', 'utf-8');
      } else {
        sendServerError(res, error.code);
      }
    } else {
      const headers = {
        'Content-Type': contentType
      };

      if (extname === '.js' || extname === '.html') {
        Object.assign(headers, CROSS_ORIGIN_ISOLATION_HEADERS);
      }

      // Vite-hashed /assets/ can cache hard; wasm-pack's /pkg/ output is NOT
      // content-hashed and the SPA shell's asset URLs change per build, so
      // both must revalidate.
      if (urlPath.startsWith('/assets/')) {
        headers['Cache-Control'] = 'public, max-age=31536000, immutable';
      } else if (urlPath.startsWith('/pkg/') || extname === '.html') {
        headers['Cache-Control'] = 'no-cache';
      }

      res.writeHead(200, headers);

      const isText = ['.html', '.js', '.json', '.css', '.svg'].includes(extname);
      res.end(content, isText ? 'utf-8' : undefined);
    }
  });
};

const server = useTLS
  ? https.createServer({ key: fs.readFileSync(keyFile), cert: fs.readFileSync(certFile) }, handler)
  : http.createServer(handler);

server.listen(PORT, () => {
  const proto = useTLS ? 'https' : 'http';
  console.log(`Server running at ${proto}://localhost:${PORT}/`);
});

// Optional plain-HTTP listener that 301s everything to the https server
// (production runs it on port 80). Only meaningful alongside TLS.
if (useTLS && redirectPort) {
  const redirectServer = http.createServer((req, res) => {
    const host = (req.headers.host || 'localhost').replace(/:\d+$/, '');
    const target = PORT === 443 ? `https://${host}` : `https://${host}:${PORT}`;
    res.writeHead(301, { Location: target + req.url });
    res.end();
  });
  // A bind failure here (EADDRINUSE, EACCES) must not take down the https
  // server — an unhandled 'error' event would crash the process.
  redirectServer.on('error', (err) => {
    console.error(`Redirect listener error on port ${redirectPort}:`, err.message);
  });
  redirectServer.listen(redirectPort, () => {
    console.log(`Redirecting http://localhost:${redirectPort}/ to https`);
  });
}
