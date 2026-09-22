// Build an accelerator string (understood by the Rust global-shortcut parser) from a key event.

const NAMED: Record<string, string> = {
  " ": "Space",
  Escape: "Escape",
  Enter: "Enter",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  Insert: "Insert",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  PrintScreen: "PrintScreen",
  ScrollLock: "ScrollLock",
  Pause: "Pause",
  CapsLock: "CapsLock",
  NumLock: "NumLock",
};

export function acceleratorFromEvent(e: KeyboardEvent): string | null {
  const key = e.key;
  if (["Control", "Shift", "Alt", "Meta", "OS", "Dead", "Unidentified"].includes(key)) return null;
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  let name: string;
  if (NAMED[key]) name = NAMED[key]!;
  else if (/^F\d{1,2}$/.test(key)) name = key;
  else if (key.length === 1) name = key.toUpperCase();
  else return null;
  // a bare letter/digit without modifiers would hijack typing everywhere
  if (mods.length === 0 && !/^F\d{1,2}$|^PrintScreen$|^ScrollLock$|^Pause$/.test(name)) return null;
  return [...mods, name].join("+");
}
