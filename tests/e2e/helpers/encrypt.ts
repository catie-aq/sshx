/**
 * Computes encrypted_zeros matching src/lib/encrypt.ts for use in e2e tests.
 *
 * Uses the same Argon2id + AES-CTR flow as the browser client, but with
 * hash-wasm (Node-friendly) instead of argon2-browser.
 */
import * as crypto from "crypto";
import { argon2id } from "hash-wasm";

const SALT =
  "This is a non-random salt for sshx.io, since we want to stretch the security of 83-bit keys!";

/**
 * Derive the AES key and encrypt 16 zero bytes, matching the browser's
 * `Encrypt.new(key).zeros()` exactly.
 */
export async function computeEncryptedZeros(key: string): Promise<Buffer> {
  const hashHex = await argon2id({
    password: key,
    salt: SALT,
    memorySize: 19 * 1024, // KiB
    iterations: 2,
    parallelism: 1,
    hashLength: 16,
    outputType: "hex",
  });

  const aesKeyBytes = Buffer.from(hashHex, "hex");

  // AES-128-CTR encrypt 16 zero bytes with an IV of 16 zero bytes
  const iv = Buffer.alloc(16, 0);
  const plaintext = Buffer.alloc(16, 0);
  const cipher = crypto.createCipheriv("aes-128-ctr", aesKeyBytes, iv);
  const encrypted = Buffer.concat([cipher.update(plaintext), cipher.final()]);

  return encrypted;
}
