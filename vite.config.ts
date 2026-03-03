import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";

import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";

const commitHash = execSync("git rev-parse --short HEAD").toString().trim();

export default defineConfig({
  define: {
    __APP_VERSION__: JSON.stringify("0.4.1-" + commitHash),
  },

  plugins: [sveltekit()],

  server: {
    port: 5173,
    strictPort: true,
    https: {
      key: readFileSync("homa-server2.gaur-toad.ts.net.key"),
      cert: readFileSync("homa-server2.gaur-toad.ts.net.crt"),
    },
    proxy: {
      "/api": {
        target: "http://homa-server2.gaur-toad.ts.net:8051",
        changeOrigin: true,
        ws: true,
        secure: false, // Allow proxy to HTTP backend
      },
    },
  },
});
