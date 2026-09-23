// Typed wrappers around every Rust command.
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type {
  AppPaths,
  ApplyResult,
  DiffResult,
  WatchCondition,
  WatchInfo,
  CaptureMode,
  DirEntry,
  EditorInit,
  HistoryItem,
  OcrOutput,
  OverlayInit,
  PinInit,
  Rect,
  RedactMatch,
  ScannedCode,
  Settings,
} from "./types";

export const currentLabel = () => getCurrentWindow().label;

type Headers = Record<string, string>;

/** invoke with a binary body; headers carry small string parameters. */
function invokeRaw<T>(cmd: string, bytes: Uint8Array, headers: Headers = {}): Promise<T> {
  // Header values must be Latin-1; percent-encode so paths and titles with any character survive.
  const encoded: Headers = {};
  for (const [k, v] of Object.entries(headers)) encoded[k] = encodeURIComponent(v);
  return invoke<T>(cmd, bytes, { headers: encoded });
}

// ---- capture / overlay ----
export const overlayInit = (label: string) => invoke<OverlayInit>("overlay_init", { label });
export const overlayReady = (label: string) => invoke<void>("overlay_ready", { label });
export const overlayIdle = (label: string) => invoke<void>("overlay_idle", { label });
export const finishCapture = (rect: Rect, windowId?: number) =>
  invoke<number>("finish_capture", { rect, windowId: windowId ?? null });
export const cancelCapture = () => invoke<void>("cancel_capture");
/** Nested UI element rects under a point in a window, outermost first (Windows; else empty). */
export const elementChain = (windowId: number, x: number, y: number) =>
  invoke<Rect[]>("element_chain", { windowId, x: Math.round(x), y: Math.round(y) });
export const triggerCapture = (mode: CaptureMode, delay?: number) =>
  invoke<void>("trigger_capture", { mode, delay: delay ?? null });

// ---- editor / output ----
export const editorInit = (label: string) => invoke<EditorInit>("editor_init", { label });
export const releaseCapture = (id: number) => invoke<void>("release_capture", { id });
export const copyImage = (png: Uint8Array) => invokeRaw<void>("copy_image", png);
export const copyText = (text: string) => invoke<void>("copy_text", { text });
export const saveImage = (png: Uint8Array, opts: { path?: string; stem?: string; format?: string } = {}) => {
  const headers: Headers = {};
  if (opts.path) headers["x-path"] = opts.path;
  if (opts.stem) headers["x-stem"] = opts.stem;
  if (opts.format) headers["x-format"] = opts.format;
  return invokeRaw<string>("save_image", png, headers);
};
export const exportTempPng = (png: Uint8Array, stem = "Screenshot") =>
  invokeRaw<string>("export_temp_png", png, { "x-stem": stem });
export const pinImage = (png: Uint8Array, x?: number, y?: number) => {
  const headers: Headers = {};
  if (x !== undefined) headers["x-x"] = String(Math.round(x));
  if (y !== undefined) headers["x-y"] = String(Math.round(y));
  return invokeRaw<number>("pin_image", png, headers);
};
export const pinInit = (label: string) => invoke<PinInit>("pin_init", { label });
export const copyCapture = (id: number, rect?: Rect) => invoke<void>("copy_capture", { id, rect: rect ?? null });
export const saveCapture = (id: number) => invoke<string>("save_capture", { id });
export const ocrCapture = (id: number, rect?: Rect) => invoke<OcrOutput>("ocr_capture", { id, rect: rect ?? null });
export const ocrPng = (png: Uint8Array) => invokeRaw<OcrOutput>("ocr_png", png);
/** OCR a rendered PNG into a table and copy it as cells (HTML + TSV). */
export const copyTable = (png: Uint8Array) =>
  invokeRaw<{ rows: number; cols: number; csv: string }>("copy_table", png);
export interface ThumbInit {
  id: number;
  width: number;
  height: number;
  pngUrl: string;
}
export const thumbInit = (label: string) => invoke<ThumbInit>("thumb_init", { label });
export const thumbAction = (id: number, action: "edit" | "pin") => invoke<void>("thumb_action", { id, action });
export const thumbDragPath = (id: number) => invoke<string>("thumb_drag_path", { id });
export const redactCapture = (id: number) => invoke<RedactMatch[]>("redact_capture", { id });
export const scanCodes = (id: number) => invoke<ScannedCode[]>("scan_codes", { id });
/** Copies image + caption (rich clipboard). Returns the caption used. */
export const copyImageRich = (png: Uint8Array, id: number) =>
  invokeRaw<string>("copy_image_rich", png, { "x-id": String(id) });

// ---- settings ----
export const getSettings = () => invoke<Settings>("get_settings");
export const setSettings = (settings: Settings) => invoke<ApplyResult>("set_settings", { settings });
export const appPaths = () => invoke<AppPaths>("app_paths");
export const openWindow = (name: "main" | "guide" | "history" | "watches") => invoke<void>("open_window", { name });
export const hideMain = () => invoke<void>("hide_main");

// ---- files ----
export const fsWrite = (path: string, bytes: Uint8Array) => invokeRaw<string>("fs_write", bytes, { "x-path": path });
export const fsWriteText = (path: string, text: string) => invoke<string>("fs_write_text", { path, text });
export const fsReadText = (path: string) => invoke<string>("fs_read_text", { path });
export const fsReadBytes = async (path: string): Promise<Uint8Array<ArrayBuffer>> =>
  new Uint8Array(await invoke<ArrayBuffer>("fs_read_bytes", { path }));
export const fsExists = (path: string) => invoke<boolean>("fs_exists", { path });
export const fsMkdir = (path: string) => invoke<void>("fs_mkdir", { path });
export const fsRemove = (path: string) => invoke<void>("fs_remove", { path });
export const fsList = (path: string) => invoke<DirEntry[]>("fs_list", { path });
export const guidesDir = () => invoke<string>("guides_dir");

// ---- guide ----
export interface PendingStep {
  id: number;
  title: string;
  width: number;
  height: number;
  pngUrl: string;
  /** the step's own title (step recorder) */
  stepTitle: string;
  /** put these steps into a new guide with this name */
  project: string | null;
}
export const guidePushStep = (png: Uint8Array, title: string) =>
  invokeRaw<number>("guide_push_step", png, { "x-title": title });
export const guidePullSteps = () => invoke<PendingStep[]>("guide_pull_steps");

// ---- history ----
export const historyList = () => invoke<HistoryItem[]>("history_list");
export const historyDir = () => invoke<string>("history_dir");
export const historyOpen = (id: number) => invoke<void>("history_open", { id });
export const historyPin = (id: number) => invoke<void>("history_pin", { id });
export const historyCopy = (id: number) => invoke<void>("history_copy", { id });
export const historySave = (id: number) => invoke<string>("history_save", { id });
export const historyDelete = (ids: number[]) => invoke<void>("history_delete", { ids });
export const historyClear = () => invoke<void>("history_clear");
export const historySetStar = (id: number, starred: boolean) => invoke<void>("history_set_star", { id, starred });
export const historyDiff = (a: number, b: number) => invoke<DiffResult>("history_diff", { a, b });
export const historyCompare = (a: number, b: number) => invoke<void>("history_compare", { a, b });
export const historyCopyRich = (id: number) => invoke<string>("history_copy_rich", { id });
export const historySetNote = (id: number, note: string) => invoke<void>("history_set_note", { id, note });

// ---- watches ----
export const watchList = () => invoke<WatchInfo[]>("watch_list");
export const watchUpdate = (id: number, intervalSecs: number, condition: WatchCondition, paused: boolean) =>
  invoke<void>("watch_update", { id, intervalSecs, condition, paused });
export const watchStop = (id: number) => invoke<void>("watch_stop", { id });
export const watchStopAll = () => invoke<void>("watch_stop_all");
export const watchSnooze = (id: number, minutes: number) => invoke<void>("watch_snooze", { id, minutes });

// ---- step recorder ----
export interface RecorderState {
  recording: boolean;
  paused: boolean;
  count: number;
}
export const stepsStart = () => invoke<void>("steps_start");
export const stepsStop = () => invoke<number>("steps_stop");
export const stepsPause = (paused: boolean) => invoke<void>("steps_pause", { paused });
export const stepsAddNow = () => invoke<void>("steps_add_now");
export const stepsState = () => invoke<RecorderState>("steps_state");
