/**
 * @file Connection token of a session's embedded IDE.
 *
 * Keep consistent with `connection_token()` in `crates/sshx/src/ide.rs`. The
 * token derives from the end-to-end encryption key, so the sshx server, which
 * relays IDE traffic, cannot open the IDE by itself.
 */

export async function ideConnectionToken(encryptionKey: string): Promise<string> {
  const data = new TextEncoder().encode(
    `sshx-ide-connection-token/v1:${encryptionKey}`,
  );
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", data));
  return Array.from(digest.slice(0, 16))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}
