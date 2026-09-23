// Floating control bar for the step recorder, scrolling capture and GIF recording. Never
// focused, excluded from screen capture, and clicks on it aren't recorded as steps.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { stepsAddNow, stepsPause, stepsState, stepsStop, type RecorderState } from "$lib/ipc";
import { showWhenReady } from "$lib/window";
import "./recorder.css";

declare global {
  interface Window {
    __QS_BAR?: { kind: "steps" | "scroll" | "gif"; text: string };
  }
}

const root = document.getElementById("app")!;
const init = window.__QS_BAR ?? { kind: "steps", text: "" };

function stepsBar() {
  root.innerHTML = `
    <div class="bar">
      <span class="rec" aria-hidden="true"></span>
      <span class="status"><strong>Recording steps</strong> · <span class="count">0 steps</span></span>
      <button class="add" title="Add a screenshot of the window under the pointer as a step">+ Step</button>
      <button class="pause" title="Clicks aren't recorded while paused">Pause</button>
      <button class="stop primary" title="Finish and open the steps as a new guide">Stop</button>
    </div>`;
  const bar = root.querySelector<HTMLElement>(".bar")!;
  const count = root.querySelector<HTMLElement>(".count")!;
  const pause = root.querySelector<HTMLButtonElement>(".pause")!;
  let paused = false;

  function render(s: RecorderState) {
    paused = s.paused;
    count.textContent = `${s.count} step${s.count === 1 ? "" : "s"}`;
    pause.textContent = s.paused ? "Resume" : "Pause";
    bar.classList.toggle("paused", s.paused);
  }

  root.querySelector<HTMLButtonElement>(".add")!.onclick = () => void stepsAddNow();
  pause.onclick = () => void stepsPause(!paused);
  root.querySelector<HTMLButtonElement>(".stop")!.onclick = () => void stepsStop();

  void stepsState().then(render);
  void listen<RecorderState>("steps://state", (e) => render(e.payload));
}

/** Scrolling capture / GIF recording: a status line updated from Rust, and Stop. */
function statusBar(kind: "scroll" | "gif", text: string) {
  root.innerHTML = `
  <div class="bar ${kind}">
    <span class="rec" aria-hidden="true"></span>
    <span class="status"></span>
    <button class="stop primary">Stop</button>
  </div>`;
  const status = root.querySelector<HTMLElement>(".status")!;
  const stop = root.querySelector<HTMLButtonElement>(".stop")!;
  status.textContent = text;
  if (kind === "scroll") stop.textContent = "Done";
  stop.title = kind === "gif" ? "Stop and save the GIF" : "Stop scrolling and keep what's captured";
  stop.onclick = () => {
    stop.disabled = true;
    void invoke("bar_stop", { kind });
  };
  void listen<string>("bar://text", (e) => (status.textContent = e.payload));
}

if (init.kind === "steps") stepsBar();
else statusBar(init.kind, init.text);
showWhenReady({ focus: false });
