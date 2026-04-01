/** Multi-selection state for canvas objects. */

export type SelectableType = "note" | "textBlock" | "widget" | "drawing";

export type SelectionItem = {
  type: SelectableType;
  id: number;
};

export type SelectionRect = {
  x1: number;
  y1: number;
  x2: number;
  y2: number;
};

/** Normalize a rect so x1<x2 and y1<y2. */
export function normalizeRect(r: SelectionRect): SelectionRect {
  return {
    x1: Math.min(r.x1, r.x2),
    y1: Math.min(r.y1, r.y2),
    x2: Math.max(r.x1, r.x2),
    y2: Math.max(r.y1, r.y2),
  };
}

/** Check if a point/rect overlaps a selection rectangle. */
export function rectContainsPoint(r: SelectionRect, x: number, y: number, w = 0, h = 0): boolean {
  const nr = normalizeRect(r);
  return !(x + w < nr.x1 || x > nr.x2 || y + h < nr.y1 || y > nr.y2);
}
