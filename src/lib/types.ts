// Mirrors of the Rust structs sent over IPC (serde camelCase).

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type CaptureMode = "region" | "window" | "fullscreen" | "repeatLast" | "ocr" | "pin" | "color";

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
}

export type ImageFormat = "png" | "jpeg";
export type AfterCapture = "editor" | "copy" | "save" | "copyAndSave";

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
}

export interface PinInit {
  id: number;
  width: number;
  height: number;
  pngUrl: string;
}

export interface OcrLine {
  text: string;
  bbox: Rect;
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
