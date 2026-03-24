/**
 * Generate VP8 video frames using ffmpeg.
 *
 * Encodes a solid-color video into IVF container format, then parses the
 * IVF headers to extract individual VP8 frames with metadata.
 */
import { execSync } from "child_process";

export interface Vp8Frame {
  data: Buffer;
  timestamp: number;
  keyframe: boolean;
}

/**
 * Generate VP8 frames for a solid-color video.
 *
 * @param color Hex color string like "0xFF0000" (red), "0x00FF00" (green), "0x0000FF" (blue)
 * @param width Frame width in pixels
 * @param height Frame height in pixels
 * @param durationSec Duration in seconds
 * @param fps Frames per second
 * @returns Array of VP8 frames parsed from the IVF container
 */
export function generateVp8Frames(
  color: string,
  width = 320,
  height = 240,
  durationSec = 0.5,
  fps = 10
): Vp8Frame[] {
  // Use ffmpeg to generate a solid-color VP8 video in IVF container format.
  // The IVF format is simple to parse and wraps raw VP8 frames.
  const ffmpegCmd = [
    "ffmpeg",
    "-f lavfi",
    `-i color=c=${color}:s=${width}x${height}:d=${durationSec}:r=${fps}`,
    `-c:v libvpx`,
    `-keyint_min 1`,
    `-g 5`, // keyframe every 5 frames
    `-quality realtime`,
    `-cpu-used 8`,
    `-f ivf`,
    `pipe:1`,
  ].join(" ");

  let ivfBuffer: Buffer;
  try {
    ivfBuffer = execSync(ffmpegCmd, {
      maxBuffer: 10 * 1024 * 1024,
      stdio: ["ignore", "pipe", "ignore"],
    });
  } catch (err) {
    throw new Error(
      `ffmpeg failed. Is ffmpeg installed with libvpx support? Error: ${err}`
    );
  }

  return parseIvf(ivfBuffer);
}

/**
 * Parse an IVF file into individual VP8 frames.
 *
 * IVF format:
 * - 32-byte file header
 * - Per frame: 12-byte header (4-byte size LE + 8-byte timestamp LE) + frame data
 *
 * VP8 keyframe detection: first byte & 0x01 === 0 means keyframe.
 */
function parseIvf(buf: Buffer): Vp8Frame[] {
  if (buf.length < 32) {
    throw new Error("IVF buffer too short for file header");
  }

  // Validate IVF signature
  const sig = buf.toString("ascii", 0, 4);
  if (sig !== "DKIF") {
    throw new Error(`Invalid IVF signature: ${sig}`);
  }

  const frames: Vp8Frame[] = [];
  let offset = 32; // skip file header

  while (offset + 12 <= buf.length) {
    const frameSize = buf.readUInt32LE(offset);
    const timestamp = Number(buf.readBigUInt64LE(offset + 4));
    offset += 12;

    if (offset + frameSize > buf.length) {
      break; // truncated frame
    }

    const data = buf.subarray(offset, offset + frameSize);
    offset += frameSize;

    // VP8 keyframe detection: bit 0 of first byte is 0 for keyframes
    const keyframe = data.length > 0 && (data[0] & 0x01) === 0;

    frames.push({
      data: Buffer.from(data),
      timestamp,
      keyframe,
    });
  }

  return frames;
}

/**
 * Check if ffmpeg is available and supports libvpx.
 */
export function checkFfmpegAvailable(): boolean {
  try {
    execSync("ffmpeg -version", { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}
