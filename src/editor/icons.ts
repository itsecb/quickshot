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
  redact: '<rect x="3" y="8" width="18" height="8" rx="1.5"/><path d="M6 12h.01M9 12h.01M12 12h.01M15 12h.01M18 12h.01"/>',
  beautify: '<rect x="7" y="7" width="10" height="10" rx="2"/><path d="M3 3h18v18H3z" opacity=".5"/>',
  spotlight: '<rect x="3" y="4" width="18" height="16" rx="2" opacity=".45"/><rect x="8" y="8" width="8" height="8" rx="2"/>',
  magnify: '<rect x="3" y="12" width="6" height="6" rx="1"/><rect x="12" y="3" width="9" height="9" rx="1.5"/><path d="M9 12l3-3" stroke-dasharray="2 2"/>',
  callout: '<path d="M4 5h16v10H10l-4 4v-4H4z"/><path d="M8 9h8M8 12h5"/>',
  table: '<rect x="3" y="4" width="18" height="16" rx="1.5"/><path d="M3 9h18M3 14h18M9 4v16M15 4v16"/>',
  qr: '<path d="M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h2v2h-2zM18 18h2v2h-2zM14 18h2M18 14h2"/>',
  ticket: '<path d="M4 7h16v4a2 2 0 000 2v4H4v-4a2 2 0 000-4z"/><path d="M9 10h6M9 14h4"/>',
};

export function icon(name: string): string {
  return `<svg viewBox="0 0 24 24">${icons[name] ?? ""}</svg>`;
}
