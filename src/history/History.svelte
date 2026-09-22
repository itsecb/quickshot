<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import { openPath } from "@tauri-apps/plugin-opener";
  import { startDrag } from "@crabnebula/tauri-plugin-drag";
  import {
    historyClear,
    historyCopy,
    historyDelete,
    historyDir,
    historyList,
    historyOpen,
    historyPin,
    historySave,
    openWindow,
  } from "$lib/ipc";
  import { isTypingTarget, primaryMod } from "$lib/keys";
  import type { HistoryItem } from "$lib/types";

  let items = $state<HistoryItem[]>([]);
  let loaded = $state(false);
  let query = $state("");
  let selected = $state<Set<number>>(new Set());
  let toast = $state<{ text: string; error: boolean } | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  function say(text: string, error = false) {
    toast = { text, error };
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = null), error ? 4000 : 1800);
  }

  async function refresh() {
    try {
      items = await historyList();
      const ids = new Set(items.map((i) => i.id));
      selected = new Set([...selected].filter((id) => ids.has(id)));
    } catch (e) {
      say(String(e), true);
    } finally {
      loaded = true;
    }
  }

  onMount(() => {
    void refresh();
    const un = listen("history://changed", () => void refresh());
    return () => void un.then((f) => f());
  });

  const timeFmt = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });
  const dayFmt = new Intl.DateTimeFormat(undefined, { weekday: "long", month: "short", day: "numeric", year: "numeric" });

  function dayLabel(d: Date): string {
    const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
    const diff = Math.round((start(new Date()) - start(d)) / 86_400_000);
    if (diff === 0) return "Today";
    if (diff === 1) return "Yesterday";
    return dayFmt.format(d);
  }

  function caption(item: HistoryItem): string {
    const s = item.source;
    if (s.title) return s.appName && !s.title.includes(s.appName) ? `${s.title} — ${s.appName}` : s.title;
    if (s.appName) return s.appName;
    return s.kind === "monitor" ? `Screen${s.monitorName ? ` (${s.monitorName})` : ""}` : "Region";
  }

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter((i) => {
      const d = new Date(i.created);
      const hay = [caption(i), i.source.kind, i.fileName, dayLabel(d), timeFmt.format(d), `${i.width}x${i.height}`]
        .join(" ")
        .toLowerCase();
      return q.split(/\s+/).every((w) => hay.includes(w));
    });
  });

  const groups = $derived.by(() => {
    const out: { label: string; items: HistoryItem[] }[] = [];
    for (const item of filtered) {
      const label = dayLabel(new Date(item.created));
      const last = out[out.length - 1];
      if (last?.label === label) last.items.push(item);
      else out.push({ label, items: [item] });
    }
    return out;
  });

  // ---- selection ----
  function onCardClick(e: MouseEvent, item: HistoryItem) {
    if (e.ctrlKey || e.metaKey || e.shiftKey) {
      const next = new Set(selected);
      if (next.has(item.id)) next.delete(item.id);
      else next.add(item.id);
      selected = next;
    } else {
      selected = new Set([item.id]);
    }
  }

  function selectedItems(): HistoryItem[] {
    return items.filter((i) => selected.has(i.id));
  }

  // ---- actions ----
  async function run(label: string, fn: () => Promise<unknown>) {
    try {
      await fn();
      if (label) say(label);
    } catch (e) {
      say(String(e), true);
    }
  }

  const edit = (item: HistoryItem) => run("", () => historyOpen(item.id));
  const pin = (item: HistoryItem) => run("", () => historyPin(item.id));
  const copy = (item: HistoryItem) => run("Copied to clipboard", () => historyCopy(item.id));
  const save = (item: HistoryItem) => run("", async () => say(`Saved ${await historySave(item.id)}`));

  async function remove(list: HistoryItem[]) {
    if (!list.length) return;
    await run(list.length > 1 ? `Deleted ${list.length} screenshots` : "Deleted", () =>
      historyDelete(list.map((i) => i.id)),
    );
  }

  async function clearAll() {
    const ok = await confirm(`Delete all ${items.length} screenshots from history? This cannot be undone.`, {
      title: "Clear history",
      kind: "warning",
    });
    if (ok) await run("History cleared", historyClear);
  }

  async function openFolder() {
    await run("", async () => openPath(await historyDir()));
  }

  // ---- drag out (to Teams, Outlook, Explorer…) ----
  let dragFrom: { x: number; y: number; item: HistoryItem } | null = null;

  function onCardDown(e: MouseEvent, item: HistoryItem) {
    if (e.button !== 0) return;
    dragFrom = { x: e.clientX, y: e.clientY, item };
  }

  function onWindowMove(e: MouseEvent) {
    if (!dragFrom || !(e.buttons & 1)) {
      dragFrom = null;
      return;
    }
    if (Math.hypot(e.clientX - dragFrom.x, e.clientY - dragFrom.y) < 6) return;
    const { item } = dragFrom;
    dragFrom = null;
    startDrag({ item: [item.path], icon: item.thumbPath, mode: "copy" }).catch((err) =>
      say(`Drag failed: ${err}`, true),
    );
  }

  // ---- keyboard ----
  function onKey(e: KeyboardEvent) {
    if (isTypingTarget(e)) {
      if (e.key === "Escape") (e.target as HTMLElement).blur();
      return;
    }
    const sel = selectedItems();
    const one = sel.length === 1 ? sel[0]! : null;
    const k = e.key.toLowerCase();
    if (primaryMod(e) && k === "f") {
      e.preventDefault();
      document.getElementById("history-search")?.focus();
    } else if (primaryMod(e) && k === "a") {
      e.preventDefault();
      selected = new Set(filtered.map((i) => i.id));
    } else if (primaryMod(e) && k === "c" && one) {
      e.preventDefault();
      void copy(one);
    } else if (primaryMod(e) && k === "s" && one) {
      e.preventDefault();
      void save(one);
    } else if (k === "enter" && one) {
      void edit(one);
    } else if (k === "p" && one) {
      void pin(one);
    } else if (k === "delete" || k === "backspace") {
      void remove(sel);
    } else if (k === "escape") {
      selected = new Set();
    }
  }
</script>

<svelte:window onkeydown={onKey} onmousemove={onWindowMove} onmouseup={() => (dragFrom = null)} />

<div class="history">
  <header>
    <h1>History</h1>
    <input id="history-search" type="text" placeholder="Search app, window title, date…" bind:value={query} />
    <span class="muted count">{filtered.length === items.length ? items.length : `${filtered.length} of ${items.length}`}</span>
    <div class="spacer"></div>
    {#if selected.size > 1}
      <button onclick={() => remove(selectedItems())}>Delete {selected.size}</button>
    {/if}
    <button onclick={openFolder}>Open folder</button>
    <button onclick={clearAll} disabled={!items.length}>Clear all</button>
    <button onclick={() => openWindow("main")} title="History settings (Output page)">Settings…</button>
  </header>

  <main>
    {#if !loaded}
      <p class="muted empty">Loading…</p>
    {:else if !items.length}
      <div class="empty">
        <p>No screenshots yet.</p>
        <p class="muted">Every capture you take shows up here, newest first. Retention is set on the Output page in Settings.</p>
      </div>
    {:else if !filtered.length}
      <p class="muted empty">Nothing matches “{query}”.</p>
    {:else}
      {#each groups as group (group.label)}
        <section>
          <h2>{group.label}</h2>
          <div class="grid">
            {#each group.items as item (item.id)}
              <div
                class="card"
                class:selected={selected.has(item.id)}
                role="button"
                tabindex="0"
                title="Double-click to edit · drag out to share"
                onclick={(e) => onCardClick(e, item)}
                ondblclick={() => edit(item)}
                onmousedown={(e) => onCardDown(e, item)}
                onkeydown={(e) => {
                  if (e.key === " ") {
                    e.preventDefault();
                    selected = new Set([item.id]);
                  }
                }}
              >
                <div class="thumb">
                  <img src={item.thumbUrl} alt="" loading="lazy" draggable="false" />
                  <div class="actions">
                    <button onclick={(e) => (e.stopPropagation(), edit(item))} title="Open in editor (Enter)">Edit</button>
                    <button onclick={(e) => (e.stopPropagation(), pin(item))} title="Pin to screen (P)">Pin</button>
                    <button onclick={(e) => (e.stopPropagation(), copy(item))} title="Copy image (Ctrl+C)">Copy</button>
                    <button onclick={(e) => (e.stopPropagation(), save(item))} title="Save to screenshots folder (Ctrl+S)">Save</button>
                    <button class="danger" onclick={(e) => (e.stopPropagation(), remove([item]))} title="Delete (Del)">✕</button>
                  </div>
                </div>
                <div class="meta">
                  <span class="caption">{caption(item)}</span>
                  <span class="muted">{timeFmt.format(new Date(item.created))} · {item.width}×{item.height}</span>
                </div>
              </div>
            {/each}
          </div>
        </section>
      {/each}
    {/if}
  </main>

  {#if toast}
    <div class="toast" class:error={toast.error}>{toast.text}</div>
  {/if}
</div>
