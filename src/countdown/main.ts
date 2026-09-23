// Delayed-capture countdown. Rust owns the timing and closes this window just before the
// screen is frozen; this page only shows the seconds left and offers a cancel button.
import { invoke } from "@tauri-apps/api/core";
import { showWhenReady } from "$lib/window";
import "./countdown.css";

declare global {
  interface Window {
    __QS_COUNTDOWN?: number;
  }
}

const total = window.__QS_COUNTDOWN ?? 3;
const started = performance.now();
const RADIUS = 17;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

const root = document.getElementById("app")!;
root.innerHTML = `
  <div class="pill">
    <span class="dial">
      <svg viewBox="0 0 40 40" aria-hidden="true">
        <circle class="track" cx="20" cy="20" r="${RADIUS}" />
        <circle class="bar" cx="20" cy="20" r="${RADIUS}" stroke-dasharray="${CIRCUMFERENCE}" />
      </svg>
      <span class="num"></span>
    </span>
    <span class="label">Capturing…</span>
    <button class="cancel" title="Cancel (or press the hotkey again)">✕</button>
  </div>`;
const num = root.querySelector<HTMLElement>(".num")!;
const bar = root.querySelector<SVGCircleElement>(".bar")!;
root.querySelector<HTMLButtonElement>(".cancel")!.onclick = () => void invoke("cancel_countdown");

function tick() {
  const left = Math.max(0, total - (performance.now() - started) / 1000);
  num.textContent = String(Math.ceil(left));
  // the ring empties smoothly over the whole countdown
  bar.style.strokeDashoffset = String(CIRCUMFERENCE * (1 - left / total));
  if (left > 0) requestAnimationFrame(tick);
}
tick();
// never take focus: the menu the user is setting up must stay open
showWhenReady({ focus: false });
