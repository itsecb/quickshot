// Guide project persistence: <guidesDir>/<slug>.snapguide/{project.json, captures/*.png}
import { fsExists, fsList, fsMkdir, fsReadBytes, fsReadText, fsRemove, fsWrite, fsWriteText, guidesDir } from "$lib/ipc";

export interface GuideStep {
  id: string;
  title: string;
  body: string;
  image: string; // file name inside captures/
  width: number;
  height: number;
}

export interface GuideProject {
  version: 1;
  title: string;
  description: string;
  createdAt: string;
  updatedAt: string;
  steps: GuideStep[];
  export: { imageMaxWidth: number; numbering: boolean };
}

export interface ProjectRef {
  dir: string;
  name: string;
  modified: string | null;
}

export const isWindows = navigator.userAgent.includes("Windows");
export const sep = isWindows ? "\\" : "/";
export const join = (...parts: string[]) => parts.join(sep).replace(isWindows ? /\\+/g : /\/+/g, sep);

export function slug(title: string): string {
  return (
    title
      .trim()
      .replace(/[<>:"/\\|?*]+/g, "")
      .replace(/\s+/g, " ")
      .slice(0, 80) || "Untitled guide"
  );
}

export function newProject(title: string): GuideProject {
  const now = new Date().toISOString();
  return { version: 1, title, description: "", createdAt: now, updatedAt: now, steps: [], export: { imageMaxWidth: 1200, numbering: true } };
}

export async function listProjects(): Promise<ProjectRef[]> {
  const dir = await guidesDir();
  const entries = await fsList(dir);
  return entries
    .filter((e) => e.isDir && e.name.endsWith(".snapguide"))
    .map((e) => ({ dir: e.path, name: e.name.replace(/\.snapguide$/, ""), modified: e.modified }));
}

export async function projectDirFor(title: string): Promise<string> {
  const base = await guidesDir();
  let dir = join(base, `${slug(title)}.snapguide`);
  let n = 2;
  while (await fsExists(dir)) dir = join(base, `${slug(title)} (${n++}).snapguide`);
  return dir;
}

export async function loadProject(dir: string): Promise<GuideProject> {
  const p = JSON.parse(await fsReadText(join(dir, "project.json"))) as GuideProject;
  if (p.version !== 1) throw new Error("unsupported guide version");
  return p;
}

export async function saveProject(dir: string, project: GuideProject): Promise<void> {
  await fsMkdir(join(dir, "captures"));
  project.updatedAt = new Date().toISOString();
  await fsWriteText(join(dir, "project.json"), JSON.stringify(project, null, 2));
}

export async function writeStepImage(dir: string, name: string, png: Uint8Array): Promise<void> {
  await fsWrite(join(dir, "captures", name), png);
}

export async function readStepImage(dir: string, name: string): Promise<Uint8Array<ArrayBuffer>> {
  return fsReadBytes(join(dir, "captures", name));
}

export async function deleteProject(dir: string): Promise<void> {
  await fsRemove(dir);
}

export async function renameProject(oldDir: string, project: GuideProject): Promise<string> {
  const newDir = await projectDirFor(project.title);
  await fsMkdir(join(newDir, "captures"));
  for (const s of project.steps) await writeStepImage(newDir, s.image, await readStepImage(oldDir, s.image));
  await saveProject(newDir, project);
  await fsRemove(oldDir);
  return newDir;
}

export function stepFileName(index: number, id: string): string {
  return `step-${String(index + 1).padStart(2, "0")}-${id}.png`;
}
