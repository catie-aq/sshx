/**
 * @file Drawing and text annotations on pasted images.
 *
 * Annotations are stored as vectors in the image's natural pixel space, so they
 * stay editable, and flattened into a PNG that the CLI saves next to the
 * original as `.sshx/images/<stem>-annotated.png`, ready to hand to Claude.
 */

export type Point = [number, number];

export type ImageAnnotation =
  | { kind: "stroke"; color: string; width: number; points: Point[] }
  | { kind: "text"; color: string; size: number; x: number; y: number; text: string };

export const ANNOTATION_COLORS = ["#f87171", "#facc15", "#34d399", "#818cf8", "#fafafa"];

/** Stroke width and font size scale with the image so they read the same at any size. */
export function strokeWidthFor(naturalWidth: number): number {
  return Math.max(2, Math.round(naturalWidth / 220));
}

export function fontSizeFor(naturalWidth: number): number {
  return Math.max(14, Math.round(naturalWidth / 32));
}

/** Parse the JSON stored on the widget, ignoring anything malformed. */
export function parseAnnotations(json: string | undefined | null): ImageAnnotation[] {
  if (!json) return [];
  try {
    const value = JSON.parse(json);
    return Array.isArray(value) ? value.filter(isAnnotation) : [];
  } catch {
    return [];
  }
}

function isAnnotation(a: any): a is ImageAnnotation {
  if (!a || typeof a.color !== "string") return false;
  if (a.kind === "stroke") return Array.isArray(a.points) && typeof a.width === "number";
  if (a.kind === "text") {
    return typeof a.text === "string" && typeof a.x === "number" && typeof a.y === "number";
  }
  return false;
}

/** SVG path data for a freehand stroke. */
export function strokePath(points: Point[]): string {
  if (points.length === 0) return "";
  const [[x0, y0], ...rest] = points;
  // A single click draws a dot.
  if (rest.length === 0) return `M${x0} ${y0}l0.01 0`;
  return `M${x0} ${y0}` + rest.map(([x, y]) => `L${x} ${y}`).join("");
}

/**
 * Mirrors `sanitize_image_name` in `crates/sshx/src/workspace.rs`: the name
 * the CLI gives the file under `.sshx/images/`.
 */
export function sanitizeImageName(name: string): string | null {
  const basename = name.split(/[\\/]/).pop() ?? name;
  const cleaned = Array.from(basename)
    .filter((c) => /[\p{L}\p{N}._-]/u.test(c))
    .join("");
  if (!cleaned || cleaned === "." || cleaned === "..") return null;
  return cleaned;
}

/** Mirrors `annotated_file_name` in `crates/sshx-server/src/web/socket.rs`. */
export function annotatedFileName(name: string): string {
  const dot = name.lastIndexOf(".");
  const stem = dot > 0 ? name.slice(0, dot) : name;
  return `${stem}-annotated.png`;
}

/**
 * Path of an image in the CLI workspace, ready to paste into a shell: absolute
 * when the workspace root is known (so it works from any directory), else
 * relative to where sshx was started.
 */
export function workspaceImagePath(
  name: string | undefined | null,
  root?: string | null,
): string | null {
  const safe = name ? sanitizeImageName(name) : null;
  if (!safe) return null;
  const relative = `.sshx/images/${safe}`;
  return root ? shellQuote(`${root.replace(/\/+$/, "")}/${relative}`) : relative;
}

/** Quote a path for a POSIX shell, only when it needs it. */
function shellQuote(path: string): string {
  return /^[\w@%+=:,./-]+$/.test(path) ? path : `'${path.replace(/'/g, `'\\''`)}'`;
}

/** Render the image with its annotations drawn on top, as a PNG. */
export async function flattenAnnotations(
  imageUrl: string,
  annotations: ImageAnnotation[],
): Promise<Blob> {
  const img = new Image();
  img.src = imageUrl;
  await img.decode();

  const canvas = document.createElement("canvas");
  canvas.width = img.naturalWidth;
  canvas.height = img.naturalHeight;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("canvas 2d context unavailable");
  ctx.drawImage(img, 0, 0);

  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  for (const a of annotations) {
    if (a.kind === "stroke") {
      ctx.strokeStyle = a.color;
      ctx.lineWidth = a.width;
      ctx.stroke(new Path2D(strokePath(a.points)));
    } else {
      ctx.font = `bold ${a.size}px sans-serif`;
      ctx.textBaseline = "top";
      ctx.lineWidth = a.size / 5;
      ctx.strokeStyle = "#18181b";
      ctx.strokeText(a.text, a.x, a.y);
      ctx.fillStyle = a.color;
      ctx.fillText(a.text, a.x, a.y);
    }
  }

  return new Promise((resolve, reject) =>
    canvas.toBlob((b) => (b ? resolve(b) : reject(new Error("toBlob failed"))), "image/png"),
  );
}
