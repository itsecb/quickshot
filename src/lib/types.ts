// Mirrors of the Rust structs sent over IPC (serde camelCase).

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type CaptureMode = "region" | "window" | "fullscreen" | "repeatLast" | "ocr" | "pin" | "color" | "qr" | "watch";

export interface MonitorInfo {
  id: number;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  scale: number;
  isPrimary: boolean;
  frameUrl: string;
}

export interface WindowInfo {
  id: number;
  title: string;
  appName: string;
  rect: Rect;
  z: number;
}

export interface OverlayInit {
  mode: CaptureMode;
  monitor: MonitorInfo;
  monitors: MonitorInfo[];
  windows: WindowInfo[];
  lastRegion: Rect | null;
  showMagnifier: boolean;
  playSound: boolean;
}

export interface CaptureSource {
  kind: string;
  appName: string;
  title: string;
  monitorName: string;
  rect: Rect;
}

export interface Hotkeys {
  region: string;
  window: string;
  fullscreen: string;
  repeatLast: string;
  ocr: string;
  pin: string;
  color: string;
  history: string;
  delayedRegion: string;
  qr: string;
  watch: string;
  recordSteps: string;
}

export type ImageFormat = "png" | "jpeg";
export type AfterCapture = "editor" | "editorAndCopy" | "copy" | "save" | "copyAndSave";

export interface EditorDefaults {
  strokeColor: string;
  strokeWidth: number;
  fontSize: number;
  fontFamily: string;
  palette: string[];
  blurAmount: number;
  badgeSize: number;
  shadow: boolean;
  shortcuts: Record<string, string>;
  beautify: Beautify;
}

export interface Beautify {
  enabled: boolean;
  padding: number;
  /** preset id or #rrggbb */
  background: string;
  radius: number;
  shadow: boolean;
}

export interface Settings {
  hotkeys: Hotkeys;
  saveDir: string | null;
  filePattern: string;
  imageFormat: ImageFormat;
  jpegQuality: number;
  afterCapture: AfterCapture;
  copyOnSave: boolean;
  showMagnifier: boolean;
  playSound: boolean;
  autostart: boolean;
  ocrLanguage: string | null;
  guidesDir: string | null;
  editor: EditorDefaults;
  historyEnabled: boolean;
  historyMaxItems: number;
  historyMaxDays: number;
  historyOcr: boolean;
  captureDelaySecs: number;
  redactPatterns: string[];
  ticketCaption: string;
  rules: Rule[];
  showThumbnail: boolean;
}

export interface Rule {
  enabled: boolean;
  name: string;
  /** comma-separated substrings of the app name */
  app: string;
  /** comma-separated substrings of the window title */
  title: string;
  skipHistory: boolean;
  autoRedact: boolean;
  autoCopy: boolean;
  saveDir: string | null;
}

export interface DiffResult {
  offsetX: number;
  offsetY: number;
  boxes: Rect[];
  changedPercent: number;
  sameSize: boolean;
}

export type WatchCondition =
  | { kind: "change"; minPercent: number }
  | { kind: "textAppears"; pattern: string }
  | { kind: "textGone"; pattern: string };

export interface WatchInfo {
  id: number;
  name: string;
  rect: Rect;
  intervalSecs: number;
  condition: WatchCondition;
  started: string;
  lastCheck: string | null;
  checks: number;
  alerts: number;
  paused: boolean;
  error: string | null;
}

export interface RedactMatch {
  kind: string;
  text: string;
  rect: Rect;
}

export interface ScannedCode {
  text: string;
  format: string;
}

export interface EditorInit {
  id: number;
  width: number;
  height: number;
  frameUrl: string;
  pngUrl: string;
  source: CaptureSource;
  created: string;
  settings: Settings;
  autoRedact: boolean;
}

export interface HistoryItem {
  id: number;
  created: string;
  width: number;
  height: number;
  fileName: string;
  source: CaptureSource;
  /** null = not read yet, "" = no text found */
  ocrText: string | null;
  starred: boolean;
  note: string;
  thumbUrl: string;
  pngUrl: string;
  path: string;
  thumbPath: string;
}

export interface PinInit {
  id: number;
  width: number;
  height: number;
  pngUrl: string;
}

export interface OcrWord {
  text: string;
  bbox: Rect;
}

export interface OcrLine {
  text: string;
  bbox: Rect;
  /** word boxes where the OS engine provides them (Windows); empty elsewhere */
  words: OcrWord[];
}

export interface OcrOutput {
  text: string;
  lines: OcrLine[];
}

export interface AppPaths {
  saveDir: string;
  guidesDir: string;
  configDir: string;
  version: string;
  platform: string;
}

export interface DirEntry {
  name: string;
  path: string;
  isDir: boolean;
  modified: string | null;
}

export interface ApplyResult {
  hotkeyConflicts: string[];
}
