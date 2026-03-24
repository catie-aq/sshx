/**
 * Simple HTTP server that serves a solid-color HTML page.
 * Used as a test fixture for streaming validation.
 */
import * as http from "http";
import * as net from "net";

export interface ColorServer {
  port: number;
  url: string;
  close: () => Promise<void>;
}

/**
 * Start a minimal HTTP server that serves a single-page HTML document
 * with a solid background color.
 *
 * @param color CSS color value (e.g. "#FF0000", "green", "rgb(0,0,255)")
 */
export async function startColorServer(color: string): Promise<ColorServer> {
  const html = `<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><title>Color Test</title></head>
<body style="margin:0;background:${color};width:100vw;height:100vh"></body>
</html>`;

  const server = http.createServer((_req, res) => {
    res.writeHead(200, { "Content-Type": "text/html" });
    res.end(html);
  });

  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      const addr = server.address() as net.AddressInfo;
      resolve({
        port: addr.port,
        url: `http://127.0.0.1:${addr.port}`,
        close: () =>
          new Promise<void>((res) => server.close(() => res())),
      });
    });
  });
}
