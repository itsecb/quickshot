import { fsWriteText } from "$lib/ipc";
import { pngToBase64 } from "$lib/image";
import { join, readStepImage, slug, type GuideProject } from "../project";

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** Minimal markdown: paragraphs, line breaks, **bold**, `code`, bullet lists. */
export function miniMarkdown(text: string): string {
  const blocks = text.trim().split(/\n\s*\n/);
  return blocks
    .map((b) => {
      const lines = b.split("\n");
      const inline = (l: string) =>
        esc(l)
          .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
          .replace(/`([^`]+)`/g, "<code>$1</code>");
      if (lines.every((l) => /^\s*[-*]\s+/.test(l))) {
        return `<ul>${lines.map((l) => `<li>${inline(l.replace(/^\s*[-*]\s+/, ""))}</li>`).join("")}</ul>`;
      }
      if (lines.every((l) => /^\s*\d+[.)]\s+/.test(l))) {
        return `<ol>${lines.map((l) => `<li>${inline(l.replace(/^\s*\d+[.)]\s+/, ""))}</li>`).join("")}</ol>`;
      }
      return `<p>${lines.map(inline).join("<br>")}</p>`;
    })
    .join("\n");
}

export async function exportHtml(dir: string, project: GuideProject, outDir: string): Promise<string> {
  const steps: string[] = [];
  for (const [i, s] of project.steps.entries()) {
    const b64 = pngToBase64(await readStepImage(dir, s.image));
    const title = esc(s.title || `Step ${i + 1}`);
    steps.push(`<section class="step">
  <h2>${project.export.numbering ? `<span class="n">${i + 1}</span>` : ""}${title}</h2>
  ${miniMarkdown(s.body)}
  <img src="data:image/png;base64,${b64}" alt="${title}" width="${s.width}" height="${s.height}">
</section>`);
  }
  const html = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>${esc(project.title)}</title>
<style>
  body{font-family:-apple-system,"Segoe UI",Helvetica,Arial,sans-serif;max-width:${project.export.imageMaxWidth + 80}px;margin:40px auto;padding:0 24px;color:#1d1f23;line-height:1.5}
  h1{font-size:28px;margin-bottom:6px} .desc{color:#555;margin-bottom:28px}
  .step{margin:36px 0;padding-top:12px;border-top:1px solid #e3e5e8}
  h2{font-size:18px;display:flex;align-items:center;gap:10px}
  .n{display:inline-flex;align-items:center;justify-content:center;width:28px;height:28px;border-radius:50%;background:#2f6fe4;color:#fff;font-size:14px}
  img{max-width:100%;height:auto;border:1px solid #d5d8de;border-radius:6px;box-shadow:0 2px 8px rgba(0,0,0,.08)}
  code{background:#f0f1f3;padding:1px 4px;border-radius:3px;font-size:.92em}
  @media print{.step{break-inside:avoid}}
</style></head><body>
<h1>${esc(project.title)}</h1>
${project.description.trim() ? `<div class="desc">${miniMarkdown(project.description)}</div>` : ""}
${steps.join("\n")}
</body></html>`;
  const path = join(outDir, `${slug(project.title)}.html`);
  await fsWriteText(path, html);
  return path;
}
