import { fsMkdir, fsWrite, fsWriteText } from "$lib/ipc";
import { join, readStepImage, type GuideProject } from "../project";

export function exportFileName(index: number): string {
  return `step-${String(index + 1).padStart(2, "0")}.png`;
}

export async function exportMarkdown(dir: string, project: GuideProject, outDir: string): Promise<string> {
  const imgDir = join(outDir, "images");
  await fsMkdir(imgDir);
  const lines: string[] = [`# ${project.title}`, ""];
  if (project.description.trim()) lines.push(project.description.trim(), "");
  for (const [i, s] of project.steps.entries()) {
    const file = exportFileName(i);
    await fsWrite(join(imgDir, file), await readStepImage(dir, s.image));
    const heading = project.export.numbering ? `## ${i + 1}. ${s.title || "Step " + (i + 1)}` : `## ${s.title || "Step " + (i + 1)}`;
    lines.push(heading, "");
    if (s.body.trim()) lines.push(s.body.trim(), "");
    lines.push(`![${s.title || "Step " + (i + 1)}](images/${file})`, "");
  }
  const path = join(outDir, "README.md");
  await fsWriteText(path, lines.join("\n"));
  return path;
}
