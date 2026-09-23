<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { historyCompare, openWindow, watchSnooze, watchStop } from "$lib/ipc";
  import type { Rect } from "$lib/types";

  interface AlertInit {
    watchId: number;
    name: string;
    reason: string;
    beforeId: number;
    afterId: number;
    afterUrl: string;
    width: number;
    height: number;
    boxes: Rect[];
  }

  const init = (window as unknown as { __QS_ALERT?: AlertInit }).__QS_ALERT;
  const win = getCurrentWindow();
  let leaving = $state(false);
  let note = $state(init?.reason ?? "");
  const time = new Intl.DateTimeFormat(undefined, { timeStyle: "short" }).format(new Date());

  async function close() {
    if (leaving) return;
    leaving = true;
    await new Promise((r) => setTimeout(r, 180));
    await win.close();
  }

  async function act(fn: () => Promise<unknown>) {
    try {
      await fn();
      await close();
    } catch (e) {
      note = String(e);
    }
  }
</script>

{#if init}
  <!-- no auto-hide: an alert should wait for you -->
  <div class="card alert" class:leaving role="alertdialog" aria-label={`Watch alert: ${init.name}`}>
    <div class="head">
      <span class="bell" aria-hidden="true">!</span>
      <span class="note" title={init.name}>{init.name}</span>
      <span class="muted time">{time}</span>
      <button class="icon-btn" onclick={close} title="Dismiss" aria-label="Dismiss">✕</button>
    </div>
    <div class="reason">{note}</div>
    <div class="shot">
      <div class="frame" style={`aspect-ratio:${init.width}/${init.height}`}>
        <img src={init.afterUrl} alt="After" draggable="false" />
        <svg viewBox={`0 0 ${init.width} ${init.height}`} preserveAspectRatio="none">
          {#each init.boxes as b, i (i)}
            <rect x={b.x} y={b.y} width={b.width} height={b.height} />
          {/each}
        </svg>
      </div>
    </div>
    <div class="actions">
      <button class="primary-btn" onclick={() => act(() => historyCompare(init.beforeId, init.afterId))}>Compare</button>
      <button onclick={() => act(() => watchSnooze(init.watchId, 10))} title="No alerts from this watch for 10 minutes">Snooze</button>
      <button onclick={() => act(() => watchStop(init.watchId))} title="Stop watching this region">Stop</button>
      <button onclick={() => act(() => openWindow("watches"))} title="Change what triggers alerts">Settings</button>
    </div>
  </div>
{/if}
