/** Undo/redo history stack for canvas operations. */

export type HistoryAction =
  | { type: "createNote"; id: number; data: any }
  | { type: "deleteNote"; id: number; data: any }
  | { type: "updateNote"; id: number; before: any; after: any }
  | { type: "createTextBlock"; id: number; data: any }
  | { type: "deleteTextBlock"; id: number; data: any }
  | { type: "updateTextBlock"; id: number; before: any; after: any }
  | { type: "createDrawing"; id: number; data: any }
  | { type: "deleteDrawing"; id: number; data: any }
  | { type: "moveObjects"; items: { type: string; id: number; fromX: number; fromY: number; toX: number; toY: number }[] }
  | { type: "batch"; actions: HistoryAction[] };

const MAX_HISTORY = 100;

export class UndoHistory {
  private undoStack: HistoryAction[] = [];
  private redoStack: HistoryAction[] = [];

  push(action: HistoryAction) {
    this.undoStack.push(action);
    if (this.undoStack.length > MAX_HISTORY) {
      this.undoStack.shift();
    }
    this.redoStack = [];
  }

  canUndo(): boolean {
    return this.undoStack.length > 0;
  }

  canRedo(): boolean {
    return this.redoStack.length > 0;
  }

  undo(): HistoryAction | null {
    const action = this.undoStack.pop();
    if (action) this.redoStack.push(action);
    return action ?? null;
  }

  redo(): HistoryAction | null {
    const action = this.redoStack.pop();
    if (action) this.undoStack.push(action);
    return action ?? null;
  }

  clear() {
    this.undoStack = [];
    this.redoStack = [];
  }
}
