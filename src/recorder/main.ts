// Step recorder control bar. Never focused, and clicks on it aren't recorded as steps.
import { listen } from "@tauri-apps/api/event";
import { stepsAddNow, stepsPause, stepsState, stepsStop, type RecorderState } from "$lib/ipc";
import { showWhenReady } from "$lib/window";
import "./recorder.css";

const root = document.getElementById("app")!;
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
showWhenReady({ focus: false });
