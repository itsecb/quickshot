//! "Copy table": rebuild rows and columns from OCR output so a screenshot of a grid, console
//! listing or report pastes into Excel/Sheets/Outlook as real cells. Pure logic, unit-tested.

use crate::geom::Rect;
use crate::ocr::OcrLine;

/// A positioned piece of text: a word where the OCR engine gives word boxes (Windows),
/// otherwise a whole line.
#[derive(Clone, Debug)]
struct Piece {
    text: String,
    rect: Rect,
}

fn center_y(r: &Rect) -> f64 {
    r.y as f64 + r.height as f64 / 2.0
}

/// Median of a non-empty list.
fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// Split a line of text into cells when we have no geometry: tabs, pipes, or runs of 2+ spaces.
fn split_text(line: &str) -> Vec<String> {
    let trimmed = line.trim().trim_matches('|');
    let parts: Vec<String> = if trimmed.contains('\t') {
        trimmed.split('\t').map(str::to_string).collect()
    } else if trimmed.contains('|') {
        trimmed.split('|').map(str::to_string).collect()
    } else {
        let mut cells = Vec::new();
        let mut cur = String::new();
        let mut spaces = 0;
        for ch in trimmed.chars() {
            if ch == ' ' {
                spaces += 1;
                continue;
            }
            if spaces >= 2 && !cur.is_empty() {
                cells.push(std::mem::take(&mut cur));
            } else if spaces == 1 {
                cur.push(' ');
            }
            spaces = 0;
            cur.push(ch);
        }
        if !cur.is_empty() {
            cells.push(cur);
        }
        cells
    };
    parts.into_iter().map(|c| c.trim().to_string()).collect()
}

/// Group pieces into rows by vertical position (a piece joins a row when its centre falls
/// inside the row's current vertical band).
fn group_rows(mut pieces: Vec<Piece>) -> Vec<Vec<Piece>> {
    pieces.sort_by(|a, b| center_y(&a.rect).total_cmp(&center_y(&b.rect)));
    let mut rows: Vec<(f64, f64, Vec<Piece>)> = Vec::new(); // (top, bottom, pieces)
    for p in pieces {
        let cy = center_y(&p.rect);
        match rows.last_mut() {
            Some((top, bottom, row)) if cy >= *top && cy <= *bottom => {
                *top = top.min(p.rect.y as f64);
                *bottom = bottom.max(p.rect.bottom() as f64);
                row.push(p);
            }
            _ => rows.push((p.rect.y as f64, p.rect.bottom() as f64, vec![p])),
        }
    }
    rows.into_iter()
        .map(|(_, _, mut row)| {
            row.sort_by_key(|p| p.rect.x);
            row
        })
        .collect()
}

/// Column boundaries (x positions) from gaps that line up across (almost) all rows.
fn column_boundaries(rows: &[Vec<Piece>], min_gap: f64) -> Vec<f64> {
    let (Some(x0), Some(x1)) = (
        rows.iter().flatten().map(|p| p.rect.x).min(),
        rows.iter().flatten().map(|p| p.rect.right()).max(),
    ) else {
        return Vec::new();
    };
    let width = (x1 - x0).max(1) as usize;
    // how many rows have text over each x
    let mut coverage = vec![0u32; width];
    for row in rows {
        let mut covered = vec![false; width];
        for p in row {
            let a = (p.rect.x - x0).max(0) as usize;
            let b = ((p.rect.right() - x0).max(0) as usize).min(width);
            covered[a..b].iter_mut().for_each(|c| *c = true);
        }
        for (c, hit) in coverage.iter_mut().zip(covered) {
            *c += hit as u32;
        }
    }
    // a column gap: a run where at most ~10% of rows have text (headers or spanning cells may)
    let allowed = (rows.len() as f64 * 0.1).floor() as u32;
    let mut out = Vec::new();
    let mut run_start: Option<usize> = None;
    for (i, &c) in coverage
        .iter()
        .enumerate()
        .chain(std::iter::once((width, &u32::MAX)))
    {
        if c <= allowed && i < width {
            run_start.get_or_insert(i);
        } else if let Some(s) = run_start.take() {
            if (i - s) as f64 >= min_gap {
                out.push(x0 as f64 + (s + i) as f64 / 2.0);
            }
        }
    }
    out
}

/// Rows of cells from OCR lines. Every row has the same number of columns.
pub fn rows_from_ocr(lines: &[OcrLine]) -> Vec<Vec<String>> {
    let has_words = lines.iter().any(|l| !l.words.is_empty());
    let has_boxes = lines.iter().any(|l| !l.bbox.is_empty());
    let mut rows: Vec<Vec<String>> = if has_words || has_boxes {
        let pieces: Vec<Piece> = lines
            .iter()
            .flat_map(|l| {
                if l.words.is_empty() {
                    vec![Piece {
                        text: l.text.clone(),
                        rect: l.bbox,
                    }]
                } else {
                    l.words
                        .iter()
                        .map(|w| Piece {
                            text: w.text.clone(),
                            rect: w.bbox,
                        })
                        .collect()
                }
            })
            .filter(|p| !p.text.trim().is_empty() && !p.rect.is_empty())
            .collect();
        if pieces.is_empty() {
            return Vec::new();
        }
        let grouped = group_rows(pieces);
        if !has_words {
            // line boxes only (macOS): geometry gives rows; text splitting gives cells
            grouped
                .iter()
                .map(|row| {
                    row.iter()
                        .flat_map(|p| split_text(&p.text))
                        .collect::<Vec<_>>()
                })
                .collect()
        } else {
            let heights: Vec<f64> = grouped
                .iter()
                .flatten()
                .map(|p| p.rect.height as f64)
                .collect();
            // a column gap is clearly wider than the space between words in a cell
            let min_gap = median(heights) * 0.9;
            let bounds = column_boundaries(&grouped, min_gap);
            grouped
                .iter()
                .map(|row| {
                    let mut cells = vec![String::new(); bounds.len() + 1];
                    for p in row {
                        let cx = p.rect.x as f64 + p.rect.width as f64 / 2.0;
                        let col = bounds.iter().filter(|&&b| cx > b).count();
                        if !cells[col].is_empty() {
                            cells[col].push(' ');
                        }
                        cells[col].push_str(p.text.trim());
                    }
                    cells
                })
                .collect()
        }
    } else {
        // no geometry at all (Linux tesseract text): split lines
        lines
            .iter()
            .flat_map(|l| l.text.lines())
            .filter(|l| !l.trim().is_empty())
            .map(split_text)
            .collect()
    };
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    for row in &mut rows {
        row.resize(cols, String::new());
    }
    // drop columns that ended up empty everywhere
    let keep: Vec<bool> = (0..cols)
        .map(|c| rows.iter().any(|r| !r[c].is_empty()))
        .collect();
    for row in &mut rows {
        let mut i = 0;
        row.retain(|_| {
            let k = keep[i];
            i += 1;
            k
        });
    }
    rows
}

/// Tab-separated: what Excel and Sheets paste into cells.
pub fn to_tsv(rows: &[Vec<String>]) -> String {
    rows.iter()
        .map(|r| {
            r.iter()
                .map(|c| c.replace(['\t', '\n', '\r'], " "))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn to_csv(rows: &[Vec<String>]) -> String {
    let cell = |c: &String| {
        if c.contains([',', '"', '\n', '\r']) {
            format!("\"{}\"", c.replace('"', "\"\""))
        } else {
            c.clone()
        }
    };
    rows.iter()
        .map(|r| r.iter().map(cell).collect::<Vec<_>>().join(","))
        .collect::<Vec<_>>()
        .join("\r\n")
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// HTML table for Outlook/Word/web apps; the first row is styled as a header.
pub fn to_html(rows: &[Vec<String>]) -> String {
    let mut out = String::from(
        "<table style=\"border-collapse:collapse;font-family:Segoe UI,Arial,sans-serif;font-size:12px\">",
    );
    for (i, r) in rows.iter().enumerate() {
        out.push_str("<tr>");
        for c in r {
            let (tag, style) = if i == 0 {
                (
                    "th",
                    "border:1px solid #ccc;padding:3px 8px;background:#f2f4f7;text-align:left",
                )
            } else {
                ("td", "border:1px solid #ccc;padding:3px 8px")
            };
            out.push_str(&format!(
                "<{tag} style=\"{style}\">{}</{tag}>",
                escape_html(c)
            ));
        }
        out.push_str("</tr>");
    }
    out.push_str("</table>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::OcrWord;

    /// Lay out a table as OCR words: each cell's words at fixed column x positions,
    /// ~7 px per character, rows 24 px apart.
    fn ocr_table(rows: &[&[&str]], col_x: &[i32]) -> Vec<OcrLine> {
        rows.iter()
            .enumerate()
            .map(|(r, cells)| {
                let y = 10 + r as i32 * 24;
                let mut words = Vec::new();
                for (c, cell) in cells.iter().enumerate() {
                    let mut x = col_x[c];
                    for w in cell.split(' ').filter(|w| !w.is_empty()) {
                        let width = w.chars().count() as u32 * 7;
                        words.push(OcrWord {
                            text: w.into(),
                            bbox: Rect::new(x, y, width, 14),
                        });
                        x += width as i32 + 4; // a single space
                    }
                }
                OcrLine {
                    text: cells.join(" "),
                    bbox: Rect::default(),
                    words,
                }
            })
            .collect()
    }

    #[test]
    fn rebuilds_columns_from_word_boxes() {
        let lines = ocr_table(
            &[
                &["Name", "Status", "Last seen"],
                &["srv-dc01", "Running", "2 min ago"],
                &["srv-sql02", "Stopped", "3 days ago"],
            ],
            &[10, 150, 280],
        );
        let rows = rows_from_ocr(&lines);
        assert_eq!(
            rows,
            vec![
                vec!["Name", "Status", "Last seen"],
                vec!["srv-dc01", "Running", "2 min ago"],
                vec!["srv-sql02", "Stopped", "3 days ago"],
            ]
        );
        assert_eq!(
            to_tsv(&rows).lines().nth(1),
            Some("srv-dc01\tRunning\t2 min ago")
        );
    }

    #[test]
    fn empty_cells_stay_in_their_column() {
        let lines = ocr_table(
            &[
                &["Host", "IP", "Notes"],
                &["web1", "10.0.0.5", ""],
                &["web2", "", "decom"],
            ],
            &[10, 120, 260],
        );
        let rows = rows_from_ocr(&lines);
        assert_eq!(rows[1], vec!["web1", "10.0.0.5", ""]);
        assert_eq!(rows[2], vec!["web2", "", "decom"]);
    }

    #[test]
    fn line_boxes_only_split_on_wide_spaces() {
        // macOS: one box per line, no words
        let lines = vec![
            OcrLine {
                text: "Name    Status".into(),
                bbox: Rect::new(0, 0, 200, 14),
                words: vec![],
            },
            OcrLine {
                text: "srv 01  Up".into(),
                bbox: Rect::new(0, 24, 200, 14),
                words: vec![],
            },
        ];
        assert_eq!(
            rows_from_ocr(&lines),
            vec![vec!["Name", "Status"], vec!["srv 01", "Up"]]
        );
    }

    #[test]
    fn plain_text_uses_tabs_and_pipes() {
        let lines = vec![OcrLine {
            text: "| a | b c |\nx\ty".into(),
            bbox: Rect::default(),
            words: vec![],
        }];
        assert_eq!(
            rows_from_ocr(&lines),
            vec![vec!["a", "b c"], vec!["x", "y"]]
        );
    }

    #[test]
    fn csv_and_html_escape() {
        let rows = vec![
            vec!["a,b".to_string(), "say \"hi\"".to_string()],
            vec!["<x>".to_string(), "&".to_string()],
        ];
        assert_eq!(to_csv(&rows), "\"a,b\",\"say \"\"hi\"\"\"\r\n<x>,&");
        let html = to_html(&rows);
        assert!(html.contains("<th"));
        assert!(html.contains("&lt;x&gt;") && html.contains("&amp;"));
    }
}
