import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";

import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";

const commitHash = execSync("git rev-parse --short HEAD").toString().trim();

const webPort = parseInt(process.env.SSHX_WEB_PORT || "5173", 10);
const serverPort = parseInt(process.env.SSHX_SERVER_PORT || "8051", 10);

export default defineConfig({
  define: {
    __APP_VERSION__: JSON.stringify("0.5.0-" + commitHash),
  },

  plugins: [sveltekit()],

  server: {
    port: webPort,
    strictPort: true,
    https: {
      key: readFileSync("homa-server2.burro-piranha.ts.net.key"),
      cert: readFileSync("homa-server2.burro-piranha.ts.net.crt"),
    },
    hmr: {
      host: process.env.SSHX_HOST || "homa-server2.burro-piranha.ts.net",
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
    },
  },
});
