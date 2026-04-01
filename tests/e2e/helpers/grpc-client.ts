/**
 * Node.js gRPC client wrapping SshxService + BrowserService.
 *
 * Uses @grpc/proto-loader to dynamically load the proto definition so we don't
 * need a separate code-gen step.
 */
import * as grpc from "@grpc/grpc-js";
import * as protoLoader from "@grpc/proto-loader";
import * as path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const PROTO_PATH = path.resolve(
  __dirname,
  "../../../crates/sshx-core/proto/sshx.proto"
);

let _packageDef: grpc.GrpcObject | null = null;

function getProto(): grpc.GrpcObject {
  if (_packageDef) return _packageDef;
  const def = protoLoader.loadSync(PROTO_PATH, {
    keepCase: false,
    longs: Number,
    enums: String,
    defaults: true,
    oneofs: true,
  });
  _packageDef = grpc.loadPackageDefinition(def);
  return _packageDef;
}

type ServiceClient = InstanceType<grpc.ServiceClientConstructor>;

function getSshxServiceClient(endpoint: string): ServiceClient {
  const proto = getProto() as any;
  const SshxService = proto.sshx.SshxService as grpc.ServiceClientConstructor;
  return new SshxService(endpoint, grpc.credentials.createInsecure());
}

function getBrowserServiceClient(endpoint: string): ServiceClient {
  const proto = getProto() as any;
  const BrowserService = proto.sshx
    .BrowserService as grpc.ServiceClientConstructor;
  return new BrowserService(endpoint, grpc.credentials.createInsecure());
}

/**
 * Create a session via gRPC Open().
 * Returns { name, token, key } where key is the encryption key.
 *
 * @param encryptedZeros — if provided, use these bytes as the encrypted_zeros
 *   auth proof. For browser-facing tests, compute via helpers/encrypt.ts to
 *   match what the browser will derive from the key.
 */
export async function createSession(
  endpoint: string,
  encryptedZeros?: Buffer,
): Promise<{ name: string; token: string; key: string; url: string }> {
  const client = getSshxServiceClient(endpoint);
  const key = "testkey1234567";
  const zeros = encryptedZeros ?? Buffer.alloc(16, 0);

  return new Promise((resolve, reject) => {
    client.open(
      {
        origin: `http://${endpoint}`,
        encryptedZeros: zeros,
        name: "e2e-test",
        writePasswordHash: null,
      },
      (err: grpc.ServiceError | null, resp: any) => {
        if (err) return reject(err);
        resolve({
          name: resp.name,
          token: resp.token,
          key,
          url: resp.url,
        });
      }
    );
  });
}

/**
 * Join a browser stream via BrowserService.Join().
 * Returns { vid }.
 */
export async function browserJoin(
  endpoint: string,
  sessionName: string,
  token: string
): Promise<{ vid: number }> {
  const client = getBrowserServiceClient(endpoint);
  return new Promise((resolve, reject) => {
    client.join(
      {
        sessionName,
        token,
        width: 320,
        height: 240,
      },
      (err: grpc.ServiceError | null, resp: any) => {
        if (err) return reject(err);
        resolve({ vid: resp.vid });
      }
    );
  });
}

export interface VideoFrameData {
  data: Buffer;
  timestamp: number;
  keyframe: boolean;
}

/**
 * Stream video frames to the server via BrowserService.Stream().
 * Returns a writable stream (call stream.write({...}) and stream.end()).
 */
export function browserStream(
  endpoint: string,
  vid: number,
  frames: VideoFrameData[]
): Promise<void> {
  const client = getBrowserServiceClient(endpoint);

  return new Promise((resolve, reject) => {
    const call = client.stream();

    call.on("error", (err: Error) => {
      // CANCELLED is expected when we end the stream
      if ((err as any).code === grpc.status.CANCELLED) return;
      reject(err);
    });
    call.on("end", () => resolve());

    // Send all frames
    for (const frame of frames) {
      call.write({
        frame: {
          vid,
          data: frame.data,
          timestamp: frame.timestamp,
          keyframe: frame.keyframe,
        },
      });
    }

    // Keep the stream open briefly so the server processes the frames
    setTimeout(() => {
      call.end();
      resolve();
    }, 500);
  });
}

/**
 * Simulate an IDE-capable CLI client by opening a persistent gRPC Channel
 * stream with ",ide" appended to the Hello message. This causes the server to
 * set ide_available=true for the session.
 *
 * Returns a cleanup function that ends the stream (setting ide_available=false).
 * Keep the returned handle alive for the duration of the test.
 */
export function simulateIdeCli(
  endpoint: string,
  name: string,
  token: string
): () => void {
  const client = getSshxServiceClient(endpoint);
  const call = client.channel();

  call.on("error", (err: Error) => {
    // CANCELLED is expected when we end the stream
    if ((err as any).code === grpc.status.CANCELLED) return;
    if ((err as any).code === grpc.status.UNAVAILABLE) return;
    console.warn("simulateIdeCli stream error:", err.message);
  });

  // Send the Hello message with the "ide" flag.
  // For proto oneof fields, the field name is set directly at the top level
  // (same pattern as BrowserUpdate: { frame: {...} } not { browserMessage: { frame: ... } }).
  call.write({ hello: `${name},${token},ide` });

  return () => {
    call.end();
  };
}
