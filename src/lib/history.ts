/**
 * @file Undo/redo history for canvas operations.
 *
 * Actions are recorded per browser tab and replayed as ordinary protocol
 * messages, so other users simply see the change. Objects keep their IDs: an
 * undone deletion is put back under the same ID, which keeps later history
 * entries pointing at the right object.
 */

/** Kinds of canvas state that can be undone. */
export type ObjectKind =
  | "note"
  | "textBlock"
  | "drawing"
  | "widget"
  | "slide"
  | "slideOrder"
  | "shell"
  | "shellName"
  | "video";

export type HistoryAction =
  | { type: "create"; kind: ObjectKind; id: number; data: any }
  | { type: "delete"; kind: ObjectKind; id: number; data: any }
  | { type: "update"; kind: ObjectKind; id: number; before: any; after: any }
  | { type: "batch"; actions: HistoryAction[] };

export type PushOptions = {
  /** Merge into the previous update of the same object and fields (typing). */
  coalesce?: boolean;
  /** Merge with the previous entry of the same group into one step (paste). */
  group?: string;
};

type Entry = { action: HistoryAction; time: number; group?: string };

const MAX_HISTORY = 200;
const COALESCE_MS = 1500;
const GROUP_MS = 2000;

/** Deep copy without `undefined` fields, safe to keep and to send back. */
export function plain<T>(value: T): T {
  return value === undefined ? value : JSON.parse(JSON.stringify(value));
}

function same(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Top-level fields that differ between two versions of an object. */
export function changedKeys(before: unknown, after: unknown): string[] {
  if (!isRecord(before) || !isRecord(after)) return same(before, after) ? [] : ["*"];
  const keys = new Set([...Object.keys(before), ...Object.keys(after)]);
  return [...keys].filter((k) => !same(before[k], after[k]));
}

/**
 * Apply the change `from → to` onto the current value, touching only the
 * fields that change, so concurrent edits to other fields survive an undo.
 */
export function applyChange<T>(current: T, from: T, to: T): T {
  if (!isRecord(current) || !isRecord(from) || !isRecord(to)) return plain(to);
  const result: Record<string, unknown> = { ...current };
  for (const k of changedKeys(from, to)) {
    if (k in to) result[k] = plain(to[k]);
    else delete result[k];
  }
  return result as T;
}

export class UndoHistory {
  private undoStack: Entry[] = [];
  private redoStack: Entry[] = [];

  push(action: HistoryAction, opts: PushOptions = {}) {
    if (action.type === "update" && changedKeys(action.before, action.after).length === 0) return;
    if (action.type === "batch" && action.actions.length === 0) return;

    const now = Date.now();
    const top = this.undoStack[this.undoStack.length - 1];

    if (opts.coalesce && top && now - top.time < COALESCE_MS && action.type === "update") {
      const prev = top.action;
      if (
        prev.type === "update" &&
        prev.kind === action.kind &&
        prev.id === action.id &&
        same(changedKeys(prev.before, prev.after), changedKeys(prev.before, action.after))
      ) {
        prev.after = action.after;
        top.time = now;
        this.redoStack = [];
        return;
      }
    }

    if (opts.group && top && top.group === opts.group && now - top.time < GROUP_MS) {
      const actions = top.action.type === "batch" ? top.action.actions : [top.action];
      top.action = { type: "batch", actions: [...actions, action] };
      top.time = now;
      this.redoStack = [];
      return;
    }

    this.undoStack.push({ action, time: now, group: opts.group });
    if (this.undoStack.length > MAX_HISTORY) this.undoStack.shift();
    this.redoStack = [];
  }

  canUndo(): boolean {
    return this.undoStack.length > 0;
  }

  canRedo(): boolean {
    return this.redoStack.length > 0;
  }

  undo(): HistoryAction | null {
    const entry = this.undoStack.pop();
    if (!entry) return null;
    // A finished step never merges with what comes next.
    entry.time = 0;
    this.redoStack.push(entry);
    return entry.action;
  }

  redo(): HistoryAction | null {
    const entry = this.redoStack.pop();
    if (!entry) return null;
    entry.time = 0;
    this.undoStack.push(entry);
    return entry.action;
  }

  clear() {
    this.undoStack = [];
    this.redoStack = [];
  }
}

type PendingCreate = {
  kind: ObjectKind;
  matches: (data: any) => boolean;
  onCreated: (id: number, data: any) => void;
  expires: number;
};

/**
 * Pairs objects this tab asked the server to create with the IDs the server
 * gave them, by matching the next new object of that kind against what was sent.
 */
export class CreateTracker {
  private pending: PendingCreate[] = [];

  expect(
    kind: ObjectKind,
    matches: (data: any) => boolean,
    onCreated: (id: number, data: any) => void,
    ttlMs = 5000,
  ) {
    this.pending.push({ kind, matches, onCreated, expires: Date.now() + ttlMs });
  }

  /** Report an object that just appeared; calls back if this tab created it. */
  notice(kind: ObjectKind, id: number, data: any) {
    const now = Date.now();
    this.pending = this.pending.filter((p) => p.expires > now);
    const index = this.pending.findIndex((p) => p.kind === kind && p.matches(data));
    if (index === -1) return;
    const [match] = this.pending.splice(index, 1);
    match.onCreated(id, data);
  }
}
