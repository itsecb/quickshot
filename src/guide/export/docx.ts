import { AlignmentType, Document, HeadingLevel, ImageRun, Packer, Paragraph, TextRun } from "docx";
import { fsWrite } from "$lib/ipc";
import { join, readStepImage, slug, type GuideProject } from "../project";

const PAGE_WIDTH_PX = 624; // 6.5in printable width at 96 dpi

function bodyParagraphs(text: string): Paragraph[] {
  return text
    .trim()
    .split(/\n\s*\n/)
    .filter(Boolean)
    .flatMap((block) => {
      const lines = block.split("\n");
      if (lines.every((l) => /^\s*[-*]\s+/.test(l))) {
        return lines.map((l) => new Paragraph({ text: l.replace(/^\s*[-*]\s+/, ""), bullet: { level: 0 } }));
      }
      return [
        new Paragraph({
          children: lines.flatMap((l, i) => [...(i ? [new TextRun({ break: 1 })] : []), new TextRun(l)]),
          spacing: { after: 120 },
        }),
      ];
    });
}

export async function exportDocx(dir: string, project: GuideProject, outDir: string): Promise<string> {
  const children: Paragraph[] = [new Paragraph({ text: project.title, heading: HeadingLevel.TITLE })];
  if (project.description.trim()) children.push(...bodyParagraphs(project.description));
  for (const [i, s] of project.steps.entries()) {
    const heading = project.export.numbering ? `${i + 1}. ${s.title || "Step " + (i + 1)}` : s.title || `Step ${i + 1}`;
    children.push(new Paragraph({ text: heading, heading: HeadingLevel.HEADING_2, spacing: { before: 320, after: 120 } }));
    if (s.body.trim()) children.push(...bodyParagraphs(s.body));
    const png = await readStepImage(dir, s.image);
    const scale = Math.min(1, PAGE_WIDTH_PX / s.width, 700 / s.height);
    children.push(
      new Paragraph({
        alignment: AlignmentType.LEFT,
        spacing: { after: 200 },
        children: [
          new ImageRun({
            type: "png",
            data: png,
            transformation: { width: Math.round(s.width * scale), height: Math.round(s.height * scale) },
            altText: { title: heading, description: heading, name: `step-${i + 1}` },
          }),
        ],
      }),
    );
  }
  const doc = new Document({
    creator: "QuickShot",
    title: project.title,
    styles: { default: { document: { run: { font: "Calibri", size: 22 } } } },
    sections: [{ children }],
  });
  const blob = await Packer.toBlob(doc);
  const path = join(outDir, `${slug(project.title)}.docx`);
  await fsWrite(path, new Uint8Array(await blob.arrayBuffer()));
  return path;
}
