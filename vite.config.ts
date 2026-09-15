import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";

import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";

const commitHash = execSync("git rev-parse --short HEAD").toString().trim();

const webPort = parseInt(process.env.SSHX_WEB_PORT || "5173", 10);
const serverPort = parseInt(process.env.SSHX_SERVER_PORT || "8051", 10);
const webHost = process.env.SSHX_WEB_HOST || "vps.burro-piranha.ts.net";

function loadTls() {
  const keyPath = process.env.TLS_KEY || "";
  const certPath = process.env.TLS_CERT || "";
  try {
    if (!keyPath || !certPath) return undefined;
    return { key: readFileSync(keyPath), cert: readFileSync(certPath) };
  } catch {
    return undefined;
  }
}
const tls = loadTls();

export default defineConfig({
  define: {
    __APP_VERSION__: JSON.stringify("0.5.0-" + commitHash),
  },

  plugins: [sveltekit()],

  optimizeDeps: {
    include: ["@tiptap/core", "@tiptap/starter-kit", "@tiptap/extension-link"],
  },

  server: {
    host: "0.0.0.0",
    port: webPort,
    strictPort: true,
    allowedHosts: [webHost],
    https: tls,
    hmr: {
      host: webHost,
      port: webPort,
      protocol: "wss",
    },
    proxy: {
      "/api": {
        target: `http://127.0.0.1:${serverPort}`,
        changeOrigin: true,
        ws: true,
        secure: false, // Allow proxy to HTTP backend
      },
      "/uploads": {
        target: `http://127.0.0.1:${serverPort}`,
        changeOrigin: true,
        secure: false,
      },
      "/ide": {
        target: `http://127.0.0.1:${serverPort}`,
        changeOrigin: true,
        ws: true,
        secure: false,
      },
    },
  },
});
