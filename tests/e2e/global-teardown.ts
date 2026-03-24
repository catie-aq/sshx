import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

export default async function globalTeardown() {
  const pid = process.env.__SSHX_SERVER_PID;
  if (pid) {
    console.log(`[global-teardown] Killing sshx-server (PID ${pid})...`);
    try {
      process.kill(Number(pid), "SIGTERM");
    } catch {
      // already dead
    }
  }

  // Clean up the port file
  const portFile = path.join(__dirname, ".server-port");
  try {
    fs.unlinkSync(portFile);
  } catch {
    // fine
  }
}
