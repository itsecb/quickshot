<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import { openPath } from "@tauri-apps/plugin-opener";
  import { startDrag } from "@crabnebula/tauri-plugin-drag";
  import {
    historyClear,
    historyCompare,
    historyCopy,
    historyCopyRich,
    historyDelete,
    historyDir,
    historyList,
    historyOpen,
    historyPin,
    historySave,
    historySetNote,
    historySetStar,
    openWindow,
  } from "$lib/ipc";
  import { isTypingTarget, primaryMod } from "$lib/keys";
  import type { HistoryItem } from "$lib/types";

  let items = $state<HistoryItem[]>([]);
  let loaded = $state(false);
  let query = $state("");
  let starredOnly = $state(false);
  let editingNote = $state<number | null>(null);
  let noteDraft = $state("");
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
    const un = [
      listen("history://changed", () => void refresh()),
      // background OCR finished one entry: patch it in place instead of reloading everything
      listen<{ id: number; text: string }>("history://ocr", (ev) => {
        const item = items.find((i) => i.id === ev.payload.id);
        if (item) item.ocrText = ev.payload.text;
      }),
    ];
    return () => un.forEach((p) => void p.then((f) => f()));
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

  function tagsOf(note: string): string[] {
    return [...note.matchAll(/#([\p{L}\p{N}_-]+)/gu)].map((m) => m[1]!.toLowerCase());
  }

  /** Everything searchable except the OCR text, lower-cased. */
  function metaText(i: HistoryItem): string {
    const d = new Date(i.created);
    return [caption(i), i.source.kind, i.fileName, dayLabel(d), timeFmt.format(d), `${i.width}x${i.height}`, i.note]
      .join(" ")
      .toLowerCase();
  }

  const queryWords = $derived(query.trim().toLowerCase().split(/\s+/).filter(Boolean));

  const filtered = $derived.by(() => {
    return items.filter((i) => {
      if (starredOnly && !i.starred) return false;
      if (!queryWords.length) return true;
      const meta = metaText(i);
      const ocr = (i.ocrText ?? "").toLowerCase();
      return queryWords.every((w) =>
        w.startsWith("#") && w.length > 1 ? tagsOf(i.note).includes(w.slice(1)) : meta.includes(w) || ocr.includes(w),
      );
    });
  });

  /** When a search word only matched text inside the image, show where. */
  function snippet(i: HistoryItem): { before: string; hit: string; after: string } | null {
    if (!i.ocrText || !queryWords.length) return null;
    const meta = metaText(i);
    const text = i.ocrText.replace(/\s+/g, " ");
    const lower = text.toLowerCase();
    const w = queryWords.find((w) => !w.startsWith("#") && !meta.includes(w) && lower.includes(w));
    if (!w) return null;
    const at = lower.indexOf(w);
    const start = Math.max(0, at - 28);
    const end = Math.min(text.length, at + w.length + 48);
    return {
      before: (start > 0 ? "…" : "") + text.slice(start, at),
      hit: text.slice(at, at + w.length),
      after: text.slice(at + w.length, end) + (end < text.length ? "…" : ""),
    };
  }

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
  const copyRich = (item: HistoryItem) =>
    run("", async () => {
      const caption = await historyCopyRich(item.id);
      say(caption ? `Copied with caption: ${caption}` : "Copied");
    });
  const save = (item: HistoryItem) => run("", async () => say(`Saved ${await historySave(item.id)}`));

  /** Newest older capture of the same window (app + title), else same size, else the one before. */
  function previousOf(item: HistoryItem): HistoryItem | null {
    const older = items.filter((i) => i.id < item.id); // items are newest first
    const sameWindow = (i: HistoryItem) =>
      !!(item.source.appName || item.source.title) &&
      i.source.appName === item.source.appName &&
      i.source.title === item.source.title;
    return (
      older.find(sameWindow) ??
      older.find((i) => i.width === item.width && i.height === item.height) ??
      older[0] ??
      null
    );
  }

  async function compare(list: HistoryItem[]) {
    if (list.length === 2) return run("", () => historyCompare(list[0]!.id, list[1]!.id));
    if (list.length !== 1) return say("Select one screenshot (compares with its previous capture) or two", true);
    const prev = previousOf(list[0]!);
    if (!prev) return say("Nothing older to compare with", true);
    await run("", () => historyCompare(prev.id, list[0]!.id));
  }

  async function remove(list: HistoryItem[]) {
    if (!list.length) return;
    await run(list.length > 1 ? `Deleted ${list.length} screenshots` : "Deleted", () =>
      historyDelete(list.map((i) => i.id)),
    );
  }

  async function toggleStar(list: HistoryItem[]) {
    if (!list.length) return;
    const starred = !list.every((i) => i.starred);
    for (const item of list) item.starred = starred; // optimistic; the reload confirms it
    await run(starred ? "Starred (never auto-deleted)" : "Unstarred", () =>
      Promise.all(list.map((i) => historySetStar(i.id, starred))),
    );
  }

  function startNote(item: HistoryItem) {
    editingNote = item.id;
    noteDraft = item.note;
    requestAnimationFrame(() => document.getElementById(`note-${item.id}`)?.focus());
  }

  async function saveNote(item: HistoryItem) {
    if (editingNote !== item.id) return;
    editingNote = null;
    if (noteDraft.trim() === item.note) return;
    item.note = noteDraft.trim();
    await run("", () => historySetNote(item.id, noteDraft));
  }

  function onNoteKey(e: KeyboardEvent, item: HistoryItem) {
    e.stopPropagation();
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void saveNote(item);
    } else if (e.key === "Escape") {
      editingNote = null;
    }
  }

  async function clearAll() {
    const unstarred = items.filter((i) => !i.starred).length;
    const ok = await confirm(`Delete ${unstarred} screenshots from history? Starred ones are kept. This cannot be undone.`, {
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
      void (e.altKey ? copyRich(one) : copy(one));
    } else if (primaryMod(e) && k === "s" && one) {
      e.preventDefault();
      void save(one);
    } else if (k === "enter" && one) {
      void edit(one);
    } else if (k === "p" && one) {
      void pin(one);
    } else if (k === "s" && !primaryMod(e) && sel.length) {
      void toggleStar(sel);
    } else if (k === "c" && !primaryMod(e) && (sel.length === 1 || sel.length === 2)) {
      void compare(sel);
    } else if (k === "n" && one) {
      e.preventDefault();
      startNote(one);
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
    <input id="history-search" type="text" placeholder="Search text in screenshots, app, title, #tag…" bind:value={query} />
    <button class="chip" class:on={starredOnly} onclick={() => (starredOnly = !starredOnly)} title="Show only starred">★ Starred</button>
    <span class="muted count">{filtered.length === items.length ? items.length : `${filtered.length} of ${items.length}`}</span>
    <div class="spacer"></div>
    {#if selected.size === 1 || selected.size === 2}
      <button
        class="primary"
        onclick={() => compare(selectedItems())}
        title={selected.size === 2 ? "Compare the two selected screenshots (C)" : "Compare with the previous capture of the same window (C)"}
        >{selected.size === 2 ? "Compare" : "Compare with previous"}</button
      >
    {/if}
    {#if selected.size > 1}
      <button onclick={() => remove(selectedItems())}>Delete {selected.size}</button>
    {/if}
    <button onclick={openFolder}>Open folder</button>
    <button onclick={clearAll} disabled={!items.some((i) => !i.starred)}>Clear all</button>
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
      <p class="muted empty">{starredOnly && !query ? "No starred screenshots yet. Press S on a screenshot to star it." : `Nothing matches “${query}”.`}</p>
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
                  <button
                    class="star"
                    class:on={item.starred}
                    onclick={(e) => (e.stopPropagation(), toggleStar([item]))}
                    title={item.starred ? "Unstar (S)" : "Star: never auto-deleted (S)"}>{item.starred ? "★" : "☆"}</button
                  >
                  <div class="actions">
                    <button onclick={(e) => (e.stopPropagation(), edit(item))} title="Open in editor (Enter)">Edit</button>
                    <button onclick={(e) => (e.stopPropagation(), pin(item))} title="Pin to screen (P)">Pin</button>
                    <button onclick={(e) => (e.stopPropagation(), copy(item))} title="Copy image (Ctrl+C)">Copy</button>
                    <button onclick={(e) => (e.stopPropagation(), copyRich(item))} title="Copy for ticket: image + caption (Ctrl+Alt+C)">Ticket</button>
                    <button onclick={(e) => (e.stopPropagation(), save(item))} title="Save to screenshots folder (Ctrl+S)">Save</button>
                    <button onclick={(e) => (e.stopPropagation(), startNote(item))} title="Add a note or #tags (N)">Note</button>
                    <button class="danger" onclick={(e) => (e.stopPropagation(), remove([item]))} title="Delete (Del)">✕</button>
                  </div>
                </div>
                <div class="meta">
                  <span class="caption">{caption(item)}</span>
                  <span class="muted">{timeFmt.format(new Date(item.created))} · {item.width}×{item.height}</span>
                  {#if snippet(item)}
                    {@const sn = snippet(item)!}
                    <span class="snippet" title="Found in the screenshot's text">{sn.before}<mark>{sn.hit}</mark>{sn.after}</span>
                  {/if}
                  {#if editingNote === item.id}
                    <textarea
                      id={"note-" + item.id}
                      class="note-edit"
                      rows="2"
                      placeholder="Note… use #tags · Enter saves, Esc cancels"
                      bind:value={noteDraft}
                      onkeydown={(e) => onNoteKey(e, item)}
                      onblur={() => saveNote(item)}
                      onclick={(e) => e.stopPropagation()}
                      onmousedown={(e) => e.stopPropagation()}
                      ondblclick={(e) => e.stopPropagation()}
                    ></textarea>
                  {:else if item.note}
                    <span class="note">
                      {item.note.replace(/#[\p{L}\p{N}_-]+/gu, "").trim()}
                      {#each tagsOf(item.note) as tag (tag)}
                        <button class="tag" onclick={(e) => (e.stopPropagation(), (query = "#" + tag))}>#{tag}</button>
                      {/each}
                    </span>
                  {/if}
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
