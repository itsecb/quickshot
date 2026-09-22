<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { historyDiff, historyList, historyOpen } from "$lib/ipc";
  import type { DiffResult, HistoryItem } from "$lib/types";

  type Mode = "changes" | "side" | "swipe" | "blink";
  const MODES: { id: Mode; label: string; key: string }[] = [
    { id: "changes", label: "Changes", key: "1" },
    { id: "side", label: "Side by side", key: "2" },
    { id: "swipe", label: "Swipe", key: "3" },
    { id: "blink", label: "Blink", key: "4" },
  ];

  // injected by the Rust side when it opens this window
  const ids = (window as unknown as { __QS_COMPARE?: { before: number; after: number } }).__QS_COMPARE ?? {
    before: 0,
    after: 0,
  };
  let before = $state<HistoryItem | null>(null);
  let after = $state<HistoryItem | null>(null);
  let result = $state<DiffResult | null>(null);
  let error = $state<string | null>(null);
  let mode = $state<Mode>("changes");
  let dim = $state(true);
  let swipe = $state(50);
  let showBefore = $state(false);
  let blinking = $state(true);
  let area = $state<HTMLDivElement | null>(null);
  let areaW = $state(0);
  let areaH = $state(0);
  let hovered = $state<number | null>(null);

  const timeFmt = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });

  onMount(() => {
    void load();
    const ro = new ResizeObserver(() => {
      areaW = area?.clientWidth ?? 0;
      areaH = area?.clientHeight ?? 0;
    });
    if (area) ro.observe(area);
    const timer = setInterval(() => {
      if (mode === "blink" && blinking) showBefore = !showBefore;
    }, 650);
    return () => {
      ro.disconnect();
      clearInterval(timer);
    };
  });

  async function load() {
    try {
      const items = await historyList();
      before = items.find((i) => i.id === ids.before) ?? null;
      after = items.find((i) => i.id === ids.after) ?? null;
      if (!before || !after) throw new Error("One of the screenshots is no longer in history.");
      void getCurrentWindow().setTitle(`Compare — ${label(before)} → ${label(after)}`);
      result = await historyDiff(before.id, after.id);
    } catch (e) {
      error = String(e);
    }
  }

  function label(i: HistoryItem): string {
    return i.source.title || i.source.appName || (i.source.kind === "monitor" ? "Screen" : "Region");
  }

  // Fit the "after" image in the view; never upscale past native pixels.
  const side = $derived(mode === "side");
  const scale = $derived.by(() => {
    if (!after || !before || !areaW || !areaH) return 0;
    const dpr = window.devicePixelRatio || 1;
    const w = side ? after.width + before.width + 24 : after.width;
    const h = side ? Math.max(after.height, before.height) : after.height;
    return Math.min(1 / dpr, (areaW - 32) / w, (areaH - 32) / h);
  });

  // "before" drawn in "after" space: content at A(x,y) sits at B(x+dx, y+dy).
  const beforeStyle = $derived(
    before && result
      ? `left:${result.offsetX * scale}px;top:${result.offsetY * scale}px;width:${before.width * scale}px;height:${before.height * scale}px`
      : "",
  );

  const summary = $derived.by(() => {
    if (!result) return "Comparing…";
    if (!result.boxes.length) return "No visible changes";
    const n = result.boxes.length;
    const pct = result.changedPercent < 1 ? "<1" : Math.round(result.changedPercent);
    return `${n} changed area${n === 1 ? "" : "s"} · ${pct}% of the screen`;
  });

  function onKey(e: KeyboardEvent) {
    const m = MODES.find((x) => x.key === e.key);
    if (m) mode = m.id;
    else if (e.key === " " && mode === "blink") {
      e.preventDefault();
      blinking = !blinking;
    } else if (e.key === "d" || e.key === "D") dim = !dim;
    else if (e.key === "Escape") void getCurrentWindow().close();
    else if (mode === "swipe" && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
      swipe = Math.max(0, Math.min(100, swipe + (e.key === "ArrowLeft" ? -5 : 5)));
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="compare">
  <header>
    <div class="who">
      {#if before && after}
        <span class="tag before">Before</span>
        <span class="name" title={before.source.title}>{label(before)}</span>
        <span class="muted">{timeFmt.format(new Date(before.created))}</span>
        <span class="arrow">→</span>
        <span class="tag after">After</span>
        <span class="name" title={after.source.title}>{label(after)}</span>
        <span class="muted">{timeFmt.format(new Date(after.created))}</span>
      {/if}
    </div>
    <div class="modes" role="tablist">
      {#each MODES as m (m.id)}
        <button role="tab" aria-selected={mode === m.id} class:active={mode === m.id} onclick={() => (mode = m.id)} title={`${m.label} (${m.key})`}>
          {m.label}
        </button>
      {/each}
    </div>
  </header>

  <div class="bar">
    <strong class:clean={result && !result.boxes.length}>{summary}</strong>
    {#if result && (result.offsetX || result.offsetY)}
      <span class="muted">auto-aligned by {result.offsetX}, {result.offsetY} px</span>
    {/if}
    {#if result && !result.sameSize}
      <span class="warn">Different sizes: only the overlapping area is compared</span>
    {/if}
    <span class="spacer"></span>
    {#if mode === "changes"}
      <label><input type="checkbox" bind:checked={dim} /> Dim unchanged (D)</label>
    {:else if mode === "swipe"}
      <input type="range" min="0" max="100" bind:value={swipe} aria-label="Swipe position" />
    {:else if mode === "blink"}
      <button onclick={() => (blinking = !blinking)}>{blinking ? "Pause" : "Play"} (Space)</button>
      <span class="tag" class:before={showBefore} class:after={!showBefore}>{showBefore ? "Before" : "After"}</span>
    {/if}
    {#if before && after}
      <button onclick={() => historyOpen(after!.id)} title="Open the after screenshot in the editor">Edit after</button>
    {/if}
  </div>

  <div class="area" bind:this={area}>
    {#if error}
      <p class="error">{error}</p>
    {:else if before && after && scale > 0}
      {#if mode === "side"}
        <div class="pair">
          <figure>
            <img src={before.pngUrl} alt="Before" style={`width:${before.width * scale}px`} />
            <figcaption>Before</figcaption>
          </figure>
          <figure>
            <div class="stack" style={`width:${after.width * scale}px;height:${after.height * scale}px`}>
              <img src={after.pngUrl} alt="After" class="fill" />
              {#if result}
                <svg viewBox={`0 0 ${after.width} ${after.height}`} class="fill">
                  {#each result.boxes as b, i (i)}
                    <rect x={b.x} y={b.y} width={b.width} height={b.height} class="box" />
                  {/each}
                </svg>
              {/if}
            </div>
            <figcaption>After</figcaption>
          </figure>
        </div>
      {:else}
        <div class="stack" style={`width:${after.width * scale}px;height:${after.height * scale}px`}>
          <img src={after.pngUrl} alt="After" class="fill" />
          {#if mode === "swipe"}
            <!-- clip in "after" coordinates so the cut lines up with the divider even when shifted -->
            <div class="clip" style={`width:${swipe}%`}>
              <img src={before.pngUrl} alt="Before" class="abs" style={beforeStyle} />
            </div>
            <div class="divider" style={`left:${swipe}%`}><span>Before</span><span>After</span></div>
          {:else if mode === "blink" && showBefore}
            <img src={before.pngUrl} alt="Before" class="abs" style={beforeStyle} />
          {:else if mode === "changes" && result}
            <svg viewBox={`0 0 ${after.width} ${after.height}`} class="fill">
              {#if dim && result.boxes.length}
                <defs>
                  <mask id="holes">
                    <rect width="100%" height="100%" fill="white" />
                    {#each result.boxes as b, i (i)}
                      <rect x={b.x} y={b.y} width={b.width} height={b.height} fill="black" />
                    {/each}
                  </mask>
                </defs>
                <rect width="100%" height="100%" class="dim" mask="url(#holes)" />
              {/if}
              {#each result.boxes as b, i (i)}
                <rect
                  x={b.x}
                  y={b.y}
                  width={b.width}
                  height={b.height}
                  class="box"
                  class:hot={hovered === i}
                  role="presentation"
                  onmouseenter={() => (hovered = i)}
                  onmouseleave={() => (hovered = null)}
                />
              {/each}
            </svg>
          {/if}
        </div>
      {/if}
    {:else}
      <p class="muted">Loading…</p>
    {/if}
  </div>
</div>
