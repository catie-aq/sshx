import { execSync, spawn, ChildProcess } from "child_process";
import * as net from "net";
import * as path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const PROJECT_ROOT = path.resolve(__dirname, "../..");

/** Find a free port on the loopback interface. */
async function findFreePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.listen(0, "::1", () => {
      const addr = srv.address() as net.AddressInfo;
      srv.close(() => resolve(addr.port));
    });
    srv.on("error", reject);
  });
}

/** Wait until a TCP connection succeeds. */
async function waitForPort(
  port: number,
  host = "::1",
  timeoutMs = 15_000
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      await new Promise<void>((resolve, reject) => {
        const sock = net.createConnection({ port, host }, () => {
          sock.end();
          resolve();
        });
        sock.on("error", reject);
      });
      return;
    } catch {
      await new Promise((r) => setTimeout(r, 200));
    }
  }
  throw new Error(`Timed out waiting for port ${port}`);
}

export default async function globalSetup() {
  // Build the server binary (release for speed, but debug works too)
  console.log("[global-setup] Building sshx-server...");
  execSync("cargo build -p sshx-server", {
    cwd: PROJECT_ROOT,
    stdio: "inherit",
  });

  const port = await findFreePort();
  process.env.SSHX_TEST_PORT = String(port);

  console.log(`[global-setup] Starting sshx-server on port ${port}...`);
  const serverBin = path.join(
    PROJECT_ROOT,
    "target/debug/sshx-server"
  );

  const child = spawn(serverBin, ["--port", String(port)], {
    cwd: PROJECT_ROOT,
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, RUST_LOG: "info" },
  });

  // Pipe server output for debugging
  child.stdout?.on("data", (d: Buffer) =>
    process.stdout.write(`[sshx-server] ${d}`)
  );
  child.stderr?.on("data", (d: Buffer) =>
    process.stderr.write(`[sshx-server] ${d}`)
  );

  // Wait for server to be ready
  await waitForPort(port);
  console.log(`[global-setup] sshx-server ready on port ${port}`);

  // Store child PID so teardown can kill it
  process.env.__SSHX_SERVER_PID = String(child.pid);

  // Store the port in a file so tests can read it
  const fs = await import("fs");
  fs.writeFileSync(
    path.join(__dirname, ".server-port"),
    String(port)
  );
}
