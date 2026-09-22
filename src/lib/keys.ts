// Keyboard helpers shared by overlay and editor.

export interface KeyCombo {
  key: string; // lower-case KeyboardEvent.key, e.g. "a", "escape", "arrowleft"
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  meta: boolean;
}

export function comboFromEvent(e: KeyboardEvent): KeyCombo {
  return { key: e.key.toLowerCase(), ctrl: e.ctrlKey, shift: e.shiftKey, alt: e.altKey, meta: e.metaKey };
}

/** True when the event target is a text field and plain keys must not trigger tools. */
export function isTypingTarget(e: Event): boolean {
  const t = e.target as HTMLElement | null;
  if (!t) return false;
  const tag = t.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || t.isContentEditable;
}

export const isMac = navigator.platform.toLowerCase().includes("mac");

/** Ctrl on Windows/Linux, Cmd on macOS. */
export function primaryMod(e: KeyboardEvent): boolean {
  return isMac ? e.metaKey : e.ctrlKey;
}

export function formatCombo(text: string): string {
  return text
    .split("+")
    .map((p) => p.trim())
    .map((p) => (isMac && /^ctrl$/i.test(p) ? "⌘" : p))
    .join(" + ");
}
