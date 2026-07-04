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

const handler = (req, res) => {
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
        res.writeHead(500);
        res.end('Server Error: ' + error.code, 'utf-8');
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
  http.createServer((req, res) => {
    const host = (req.headers.host || 'localhost').replace(/:\d+$/, '');
    const target = PORT === 443 ? `https://${host}` : `https://${host}:${PORT}`;
    res.writeHead(301, { Location: target + req.url });
    res.end();
  }).listen(redirectPort, () => {
    console.log(`Redirecting http://localhost:${redirectPort}/ to https`);
  });
}
