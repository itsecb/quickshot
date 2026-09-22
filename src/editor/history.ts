import type { Document } from "./document";

/** Snapshot-based undo/redo. Documents are small so structural sharing via spread is enough. */
export class History {
  private past: Document[] = [];
  private future: Document[] = [];
  private limit = 120;

  constructor(public current: Document) {}

  /** Replace the document, recording the previous state. */
  commit(next: Document) {
    if (next === this.current) return;
    this.past.push(this.current);
    if (this.past.length > this.limit) this.past.shift();
    this.future = [];
    this.current = next;
  }

  /** Replace the document without recording history (live previews, load). */
  replace(next: Document) {
    this.current = next;
  }

  undo(): boolean {
    const prev = this.past.pop();
    if (!prev) return false;
    this.future.push(this.current);
    this.current = prev;
    return true;
  }

  redo(): boolean {
    const next = this.future.pop();
    if (!next) return false;
    this.past.push(this.current);
    this.current = next;
    return true;
  }

  get canUndo() {
    return this.past.length > 0;
  }
  get canRedo() {
    return this.future.length > 0;
  }
}
