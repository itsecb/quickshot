// Inline SVG paths (24x24 viewBox) for toolbar buttons.
export const icons: Record<string, string> = {
  select: '<path d="M5 3l14 8-6 2-3 6z"/>',
  arrow: '<path d="M5 19L19 5M11 5h8v8"/>',
  line: '<path d="M4 20L20 4"/>',
  rect: '<rect x="4" y="5" width="16" height="14" rx="1.5"/>',
  ellipse: '<ellipse cx="12" cy="12" rx="8.5" ry="6.5"/>',
  pen: '<path d="M4 20c4-1 3-6 6-8s6 1 10-8"/>',
  text: '<path d="M6 5h12M12 5v14M9 19h6"/>',
  highlighter: '<path d="M4 20h6M9 15l7-9 3 3-7 9zM7 17l2 2"/>',
  blur: '<rect x="4" y="4" width="16" height="16" rx="2"/><path d="M4 9h16M4 14h16M9 4v16M14 4v16" opacity=".5"/>',
  badge: '<circle cx="12" cy="12" r="8.5"/><path d="M10.5 9.5l1.5-1v7"/>',
  crop: '<path d="M7 3v14h14M3 7h14v14"/>',
  measure: '<path d="M3 17L17 3l4 4L7 21zM8 12l2 2M11 9l2 2M14 6l2 2"/>',
  undo: '<path d="M9 14l-4-4 4-4M5 10h9a5 5 0 010 10h-3"/>',
  redo: '<path d="M15 14l4-4-4-4M19 10h-9a5 5 0 000 10h3"/>',
  copy: '<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V6a2 2 0 012-2h9"/>',
  save: '<path d="M5 4h11l3 3v13H5zM8 4v5h7V4M8 20v-6h8v6"/>',
  pin: '<path d="M9 4h6l-1 6 3 3v2H7v-2l3-3zM12 15v6"/>',
  ocr: '<path d="M4 8V4h4M16 4h4v4M20 16v4h-4M8 20H4v-4M8 9h8M12 9v7"/>',
  guide: '<path d="M6 4h12v16H6zM9 8h6M9 12h6M9 16h3"/>',
  drag: '<path d="M9 5h.01M15 5h.01M9 12h.01M15 12h.01M9 19h.01M15 19h.01"/>',
  fit: '<path d="M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5"/>',
  clear: '<path d="M6 6l12 12M18 6L6 18"/>',
};

export function icon(name: string): string {
  return `<svg viewBox="0 0 24 24">${icons[name] ?? ""}</svg>`;
}
