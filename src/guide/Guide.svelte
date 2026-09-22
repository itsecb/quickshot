<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { confirm, open as openDialog } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { guidePullSteps, releaseCapture, type PendingStep } from "$lib/ipc";
  import {
    deleteProject,
    dirName,
    listProjects,
    loadProject,
    newProject,
    projectDirFor,
    readStepImage,
    renameProject,
    saveProject,
    slug,
    stepFileName,
    writeStepImage,
    type GuideProject,
    type ProjectRef,
  } from "./project";
  import { exportMarkdown } from "./export/markdown";
  import { exportHtml } from "./export/html";
  import { exportDocx } from "./export/docx";

  let projects = $state<ProjectRef[]>([]);
  let current = $state<{ dir: string; project: GuideProject } | null>(null);
  let thumbs = $state<Record<string, string>>({});
  let status = $state("");
  let isError = $state(false);
  let saveTimer: ReturnType<typeof setTimeout> | undefined;

  function say(text: string, error = false) {
    status = text;
    isError = error;
  }

  async function refreshList() {
    projects = await listProjects();
  }

  async function openProject(ref: ProjectRef) {
    await flushSave();
    const project = await loadProject(ref.dir);
    current = { dir: ref.dir, project };
    thumbs = {};
    for (const s of project.steps) void loadThumb(ref.dir, s.image);
    say(`Opened ${project.title}`);
  }

  async function loadThumb(dir: string, image: string) {
    try {
      const bytes = await readStepImage(dir, image);
      const url = URL.createObjectURL(new Blob([bytes], { type: "image/png" }));
      thumbs = { ...thumbs, [image]: url };
    } catch (e) {
      console.warn("thumb failed", e);
    }
  }

  async function createProject(title = "New guide") {
    await flushSave();
    const project = newProject(title);
    const dir = await projectDirFor(title);
    await saveProject(dir, project);
    await refreshList();
    current = { dir, project };
    thumbs = {};
  }

  function scheduleSave() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => void flushSave(), 600);
  }

  async function flushSave() {
    clearTimeout(saveTimer);
    if (!current) return;
    try {
      await saveProject(current.dir, $state.snapshot(current.project));
    } catch (e) {
      say(`Save failed: ${e}`, true);
    }
  }

  async function commitTitle() {
    if (!current) return;
    const wantDir = slug(current.project.title);
    if (current.project.title.trim() && wantDir !== dirName(current.dir)) {
      const newDir = await renameProject(current.dir, $state.snapshot(current.project));
      current = { dir: newDir, project: current.project };
      for (const s of current.project.steps) void loadThumb(newDir, s.image);
      await refreshList();
    } else {
      scheduleSave();
    }
  }

  async function ingestPending() {
    const pending = await guidePullSteps();
    if (pending.length === 0) return;
    if (!current) {
      await createProject(pending[0]!.title ? `Guide: ${pending[0]!.title}` : "New guide");
    }
    for (const p of pending) await addPendingStep(p);
    await flushSave();
    await refreshList();
    say(`Added ${pending.length} step${pending.length > 1 ? "s" : ""}`);
  }

  async function addPendingStep(p: PendingStep) {
    if (!current) return;
    const res = await fetch(p.pngUrl, { cache: "no-store" });
    const png = new Uint8Array(await res.arrayBuffer());
    void releaseCapture(p.id);
    const id = Math.random().toString(36).slice(2, 8);
    const image = stepFileName(current.project.steps.length, id);
    await writeStepImage(current.dir, image, png);
    current.project.steps.push({ id, title: "", body: "", image, width: p.width, height: p.height });
    thumbs = { ...thumbs, [image]: URL.createObjectURL(new Blob([png], { type: "image/png" })) };
  }

  function move(i: number, dir: -1 | 1) {
    if (!current) return;
    const steps = current.project.steps;
    const j = i + dir;
    if (j < 0 || j >= steps.length) return;
    [steps[i], steps[j]] = [steps[j]!, steps[i]!];
    scheduleSave();
  }

  async function removeStep(i: number) {
    if (!current) return;
    if (!(await confirm("Remove this step?", { title: "QuickShot", kind: "warning" }))) return;
    current.project.steps.splice(i, 1);
    scheduleSave();
  }

  async function removeProject() {
    if (!current) return;
    if (!(await confirm(`Delete the guide "${current.project.title}" and its images?`, { title: "QuickShot", kind: "warning", okLabel: "Delete" }))) return;
    await deleteProject(current.dir);
    current = null;
    await refreshList();
  }

  async function doExport(kind: "md" | "html" | "docx") {
    if (!current) return;
    await flushSave();
    const outDir = await openDialog({ directory: true, multiple: false, title: "Choose the export folder" });
    if (typeof outDir !== "string") return;
    try {
      say("Exporting…");
      const snap = $state.snapshot(current.project);
      const path =
        kind === "md" ? await exportMarkdown(current.dir, snap, outDir) : kind === "html" ? await exportHtml(current.dir, snap, outDir) : await exportDocx(current.dir, snap, outDir);
      say(`Exported ${path}`);
      await revealItemInDir(path);
    } catch (e) {
      say(`Export failed: ${e}`, true);
    }
  }

  onMount(() => {
    let unlisten: (() => void) | undefined;
    void (async () => {
      unlisten = await listen("guide://step-added", () => void ingestPending());
      try {
        await refreshList();
        const first = projects[0];
        if (first) await openProject(first);
        await ingestPending();
      } catch (e) {
        say(String(e), true);
      }
    })();
    window.addEventListener("beforeunload", () => void flushSave());
    return () => unlisten?.();
  });
</script>

<div class="guide">
  <aside>
    <div class="head">
      <span>Guides</span>
      <button onclick={() => createProject()}>+ New</button>
    </div>
    <ul>
      {#each projects as p (p.dir)}
        <li>
          <button class="proj" class:active={current?.dir === p.dir} onclick={() => openProject(p)}>
            <span>{p.name}</span>
            {#if p.modified}<small>{new Date(p.modified).toLocaleString()}</small>{/if}
          </button>
        </li>
      {/each}
    </ul>
  </aside>

  <main>
    {#if current}
      <div class="project-head">
        <input class="title" type="text" bind:value={current.project.title} onchange={commitTitle} placeholder="Guide title" />
        <textarea bind:value={current.project.description} oninput={scheduleSave} placeholder="Introduction (optional, Markdown-ish: **bold**, `code`, - bullets)"></textarea>
        <div class="actions">
          <label><input type="checkbox" bind:checked={current.project.export.numbering} onchange={scheduleSave} /> Number steps</label>
          <span class="spacer"></span>
          <button onclick={() => doExport("md")}>Export Markdown</button>
          <button onclick={() => doExport("html")}>Export HTML</button>
          <button onclick={() => doExport("docx")}>Export Word</button>
          <button onclick={() => revealItemInDir(current!.dir)}>Show folder</button>
          <button onclick={removeProject} style="color:var(--danger)">Delete</button>
        </div>
      </div>

      <div class="steps">
        {#if current.project.steps.length === 0}
          <div class="empty">
            No steps yet. Take a screenshot, mark it up, then press <kbd>Ctrl+E</kbd> (or the Guide button) in the editor to add it here.
          </div>
        {/if}
        {#each current.project.steps as step, i (step.id)}
          <div class="step">
            <div class="n">{i + 1}</div>
            <div class="fields">
              <input type="text" bind:value={step.title} oninput={scheduleSave} placeholder={`Step ${i + 1} title`} />
              <textarea bind:value={step.body} oninput={scheduleSave} placeholder="What to do in this step…"></textarea>
            </div>
            <div class="thumb">
              {#if thumbs[step.image]}<img src={thumbs[step.image]} alt="" />{/if}
              <div class="btns">
                <button onclick={() => move(i, -1)} disabled={i === 0} title="Move up">↑</button>
                <button onclick={() => move(i, 1)} disabled={i === current!.project.steps.length - 1} title="Move down">↓</button>
                <button onclick={() => removeStep(i)} title="Remove step">✕</button>
              </div>
            </div>
          </div>
        {/each}
      </div>
    {:else}
      <div class="empty">
        Create a guide, or add a step from the editor with <kbd>Ctrl+E</kbd> and a guide will be created for you.
      </div>
    {/if}
    <div class="status" class:error={isError}>{status}</div>
  </main>
</div>
