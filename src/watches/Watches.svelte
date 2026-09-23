<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { triggerCapture, watchList, watchSnooze, watchStop, watchStopAll, watchUpdate } from "$lib/ipc";
  import type { WatchCondition, WatchInfo } from "$lib/types";

  let watches = $state<WatchInfo[]>([]);
  let loaded = $state(false);
  // edits in progress per watch, so typing a pattern isn't overwritten by the next check's refresh
  let drafts = $state<Record<number, { kind: WatchCondition["kind"]; pattern: string; minPercent: number }>>({});

  const INTERVALS = [5, 10, 30, 60, 300, 900];
  const rel = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

  function ago(iso: string | null): string {
    if (!iso) return "not yet";
    const s = Math.round((Date.parse(iso) - Date.now()) / 1000);
    return Math.abs(s) < 60 ? rel.format(s, "second") : rel.format(Math.round(s / 60), "minute");
  }

  // a draft per watch, created outside rendering (Svelte doesn't allow state writes in markup)
  $effect(() => {
    for (const w of watches) {
      if (drafts[w.id]) continue;
      const c = w.condition;
      drafts[w.id] = {
        kind: c.kind,
        pattern: c.kind === "change" ? "" : c.pattern,
        minPercent: c.kind === "change" ? c.minPercent : 0.5,
      };
    }
  });

  function conditionOf(id: number): WatchCondition {
    const d = drafts[id]!;
    return d.kind === "change" ? { kind: "change", minPercent: d.minPercent } : { kind: d.kind, pattern: d.pattern };
  }

  async function apply(w: WatchInfo, patch: Partial<{ interval: number; paused: boolean }> = {}) {
    await watchUpdate(w.id, patch.interval ?? w.intervalSecs, conditionOf(w.id), patch.paused ?? w.paused);
  }

  async function watchAnother() {
    await triggerCapture("watch");
  }

  onMount(() => {
    void watchList().then((w) => ((watches = w), (loaded = true)));
    const un = listen<WatchInfo[]>("watches://changed", (e) => (watches = e.payload));
    const tick = setInterval(() => (watches = [...watches]), 15000); // refresh "x seconds ago"
    return () => {
      clearInterval(tick);
      void un.then((f) => f());
    };
  });
</script>

<div class="watches">
  <header>
    <h1>Watches</h1>
    <span class="muted">{watches.length ? `${watches.length} running` : ""}</span>
    <span class="spacer"></span>
    <button class="primary" onclick={watchAnother} title="Select a region to watch (also in the tray)">+ Watch a region</button>
    {#if watches.length > 1}<button onclick={() => watchStopAll()}>Stop all</button>{/if}
  </header>

  <main>
    {#if loaded && !watches.length}
      <div class="empty">
        <p>Nothing is being watched.</p>
        <p class="muted">
          Pick a dashboard tile, a status page or a log window and QuickShot checks it every few seconds. When it
          changes, or when text like <em>Failed</em> shows up, you get an alert with a before/after comparison.
        </p>
        <button class="primary" onclick={watchAnother}>Watch a region…</button>
      </div>
    {/if}
    {#each watches as w (w.id)}
      {@const d = drafts[w.id]}
      {#if d}
      <section class="watch" class:paused={w.paused}>
        <div class="title">
          <span class="dot" class:live={!w.paused && !w.error} class:err={!!w.error}></span>
          <strong title={w.name}>{w.name}</strong>
          <span class="muted size">{w.rect.width}×{w.rect.height}</span>
          <button onclick={() => apply(w, { paused: !w.paused })}>{w.paused ? "Resume" : "Pause"}</button>
          <button onclick={() => watchSnooze(w.id, 10)} title="No alerts for 10 minutes">Snooze</button>
          <button class="danger" onclick={() => watchStop(w.id)}>Stop</button>
        </div>
        <div class="row">
          <label>Alert when
            <select bind:value={d.kind} onchange={() => apply(w)}>
              <option value="change">it changes</option>
              <option value="textAppears">this text appears</option>
              <option value="textGone">this text goes away</option>
            </select>
          </label>
          {#if d.kind === "change"}
            <label title="Ignore changes smaller than this share of the region (blinking cursors, clocks)">
              by at least
              <select bind:value={d.minPercent} onchange={() => apply(w)}>
                <option value={0.2}>any bit</option>
                <option value={0.5}>0.5%</option>
                <option value={2}>2%</option>
                <option value={10}>10%</option>
              </select>
            </label>
          {:else}
            <input
              type="text"
              placeholder="e.g. Failed, Error, re:5\d\d"
              bind:value={d.pattern}
              onchange={() => apply(w)}
              onkeydown={(e) => e.key === "Enter" && apply(w)}
            />
          {/if}
          <label>every
            <select value={w.intervalSecs} onchange={(e) => apply(w, { interval: +e.currentTarget.value })}>
              {#each INTERVALS as s (s)}<option value={s}>{s < 60 ? `${s} s` : `${s / 60} min`}</option>{/each}
            </select>
          </label>
        </div>
        <div class="stats muted">
          {#if w.error}<span class="error">⚠ {w.error}</span>{:else}Checked {ago(w.lastCheck)}{/if}
          · {w.checks} checks · {w.alerts} alert{w.alerts === 1 ? "" : "s"} · alerts are saved in History (#watch)
        </div>
      </section>
      {/if}
    {/each}
  </main>
</div>
