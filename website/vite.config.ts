import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const {
  MIME_TYPES,
  CROSS_ORIGIN_ISOLATION_HEADERS,
  resolvePkgFile,
  safeUrlPath,
  trailingSlashRedirectTarget,
} = require("./serve-common.cjs");

// During `vite dev`, serve the demo's WASM artifacts (repo-root /pkg, produced
// by `npm run build:wasm`) and canonicalize trailing-slash paths, exactly like
// website/server.js does in production.
function demoAssets(): Plugin {
  return {
    name: "webtrip-demo-assets",
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const urlPath = safeUrlPath(req.url);
        if (!urlPath) {
          res.writeHead(400);
          res.end("Bad Request");
          return;
        }
        const redirect = trailingSlashRedirectTarget(urlPath);
        if (redirect) {
          res.writeHead(301, { Location: redirect });
          res.end();
          return;
        }
        const filePath = resolvePkgFile(urlPath);
        if (!filePath || !fs.existsSync(filePath)) {
          next();
          return;
        }
        const ext = path.extname(filePath).toLowerCase();
        res.writeHead(200, {
          "Content-Type": MIME_TYPES[ext] ?? "application/octet-stream",
          ...CROSS_ORIGIN_ISOLATION_HEADERS,
        });
        fs.createReadStream(filePath).pipe(res);
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), demoAssets()],
  server: {
    // The demo needs cross-origin isolation; harmless for the rest of the site.
    headers: CROSS_ORIGIN_ISOLATION_HEADERS,
  },
});
