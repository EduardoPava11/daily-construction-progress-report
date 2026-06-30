//! Pure-Rust vector PDF renderer. Reproduces the City of Edmonton Daily
//! Construction Progress Report by replaying the ORIGINAL spreadsheet geometry
//! (cell borders, merges, and label text extracted from the .xlsx) and
//! overlaying the user's data into the exact input cells. No web-sys here, so
//! it is unit-testable on the host with `cargo test`.

use std::collections::{BTreeMap, BTreeSet};

use pdf_writer::{Content, Filter, Name, Pdf, Rect, Ref, Str};
use serde_json::Value;

// ---------- page + grid geometry (calibrated to the fit-to-width original) --
const PAGE_W: f32 = 612.0;
const PAGE_H: f32 = 792.0;
const MARGIN_X: f32 = 36.0;
const MARGIN_Y: f32 = 24.0;
const COLS: i64 = 12;
const COL_W: f32 = (PAGE_W - 2.0 * MARGIN_X) / COLS as f32; // 45
const ROW_H: f32 = 11.1;
const FONT_SCALE: f32 = 0.70; // Arial pt -> rendered pt at fit-to-width scale
const ROWS_PER_PAGE: i64 = 67; // floor((792 - 2*24) / 11.1)
const FORM_TOP: f32 = PAGE_H - MARGIN_Y; // 768

// ---------- colors ----------
const BLACK: (f32, f32, f32) = (0.0, 0.0, 0.0);
const EDM_BLUE: (f32, f32, f32) = (0.0, 0.18, 0.46);
const WHITE: (f32, f32, f32) = (1.0, 1.0, 1.0);

const F_REG: Name = Name(b"F1");
const F_BOLD: Name = Name(b"F2");

// embedded original-template geometry (merges / borders / label texts)
const TEMPLATE_JSON: &str = include_str!("template_model.json");

/// Flat form data: text fields plus the set of checked boxes.
#[derive(Default, Clone)]
pub struct ReportData {
    pub text: BTreeMap<String, String>,
    pub checks: BTreeSet<String>,
}
impl ReportData {
    pub fn t(&self, k: &str) -> &str {
        self.text.get(k).map(|s| s.as_str()).unwrap_or("")
    }
    pub fn c(&self, k: &str) -> bool {
        self.checks.contains(k)
    }
}

pub struct Logo<'a> {
    pub jpeg: &'a [u8],
    pub w: i32,
    pub h: i32,
}

// ---------- parsed template ----------
struct Merge { r0: i64, c0: i64, r1: i64, c1: i64 }
struct Border { r: i64, c: i64, l: u8, t: u8, rt: u8, b: u8 }
struct Txt { r: i64, c: i64, text: String, size: f32, bold: bool, h: u8 } // h:0 left,1 center,2 right

struct Template {
    merges: Vec<Merge>,
    borders: Vec<Border>,
    texts: Vec<Txt>,
}

fn parse_template() -> Template {
    let v: Value = serde_json::from_str(TEMPLATE_JSON).expect("template json");
    let merges = v["merges"].as_array().unwrap().iter().map(|m| {
        let a = m.as_array().unwrap();
        Merge { r0: a[0].as_i64().unwrap(), c0: a[1].as_i64().unwrap(), r1: a[2].as_i64().unwrap(), c1: a[3].as_i64().unwrap() }
    }).collect();
    let borders = v["borders"].as_array().unwrap().iter().map(|b| {
        let a = b.as_array().unwrap();
        Border { r: a[0].as_i64().unwrap(), c: a[1].as_i64().unwrap(),
                 l: a[2].as_u64().unwrap() as u8, t: a[3].as_u64().unwrap() as u8,
                 rt: a[4].as_u64().unwrap() as u8, b: a[5].as_u64().unwrap() as u8 }
    }).collect();
    let texts = v["texts"].as_array().unwrap().iter().map(|t| {
        let halign = match t["h"].as_str().unwrap_or("") {
            "center" => 1u8, "right" => 2u8, _ => 0u8,
        };
        let size = t["size"].as_f64().unwrap_or(0.0) as f32;
        Txt {
            r: t["r"].as_i64().unwrap(), c: t["c"].as_i64().unwrap(),
            text: t["text"].as_str().unwrap_or("").to_string(),
            size: if size == 0.0 { 10.0 } else { size },
            bold: t["bold"].as_bool().unwrap_or(false),
            h: halign,
        }
    }).collect();
    Template { merges, borders, texts }
}

fn leak(s: String) -> &'static str {
    &*Box::leak(s.into_boxed_str())
}

// ---------- field overlay map: name -> (row, col_start, col_end) ----------
// Values are drawn left-aligned into these spreadsheet cell ranges.
fn field_cells() -> Vec<(&'static str, i64, i64, i64)> {
    let mut v = vec![
        ("date", 6, 2, 3),
        ("contractNo", 6, 5, 6),
        ("networkActivityNo", 6, 9, 11),
        ("location", 8, 3, 11),
        ("inspStart", 10, 3, 3),
        ("inspEnd", 10, 5, 5),
        ("inspector", 11, 1, 11),
        ("contrStart", 14, 3, 3),
        ("contrEnd", 14, 5, 5),
        ("contractor", 15, 1, 11),
        ("weather", 16, 3, 4),
        ("tempHigh", 16, 8, 8),
        ("tempLow", 16, 11, 11),
        ("inspectorSig", 72, 4, 7),
        ("inspectorSigDate", 72, 10, 11),
        ("pmSig", 74, 4, 7),
        ("pmSigDate", 74, 10, 11),
    ];
    // workforce crew/equipment: 5 columns x 8 rows (sheet rows 23..30)
    for col in 0..5i64 {
        for row in 0..8i64 {
            // leaked &'static via Box::leak so the tuple can hold &'static str
            let crew = leak(format!("wf_{col}_{row}_crew"));
            let equip = leak(format!("wf_{col}_{row}_equip"));
            let base = 1 + col * 2;
            v.push((crew, 23 + row, base, base));
            v.push((equip, 23 + row, base + 1, base + 1));
        }
    }
    // work-in-progress descriptions: rows 33..40, cols 1..9
    for i in 0..8i64 {
        let n = leak(format!("wip_{i}_text"));
        v.push((n, 33 + i, 1, 9));
    }
    v
}

// checkbox cells: (field_name, row, col)
fn checkbox_cells() -> Vec<(&'static str, i64, i64)> {
    let mut v = Vec::new();
    for i in 0..8i64 {
        let n = leak(format!("wip_{i}_lab"));
        v.push((n, 33 + i, 10)); // column J
    }
    // checklist left column (col E = 5), rows 55..69
    let left = [
        "barr_arrival","barr_departure","barr_secure","exc_truckbox","fill_lift","sub_elev",
        "cb_grade","cb_elev","cb_thick","conc_straight","conc_elev","conc_cross","conc_forms",
        "sc_grade","sc_lift",
    ];
    for (i, name) in left.iter().enumerate() {
        v.push((*name, 55 + i as i64, 5));
    }
    // checklist right column (col I = 9), rows 55..67
    let right = [
        "dr_pipe","dr_grade","dr_elev","ls_topsoil","pv_grade","pv_straight","pv_joints",
        "as_notify","as_lift","as_spread","fdr_grade","fdr_cement","fdr_visual",
    ];
    for (i, name) in right.iter().enumerate() {
        v.push((*name, 55 + i as i64, 9));
    }
    v
}

// ---------- text metrics (Arial ~ Helvetica) ----------
use crate::metrics::{HELVETICA, HELVETICA_BOLD};
fn width_of(s: &str, bold: bool, size: f32) -> f32 {
    let table = if bold { &HELVETICA_BOLD } else { &HELVETICA };
    let mut w: u32 = 0;
    for ch in s.chars() {
        let c = ch as usize;
        w += if c < 256 { table[c] } else { table[b'?' as usize] } as u32;
    }
    (w as f32) / 1000.0 * size
}
fn winansi(s: &str) -> Vec<u8> {
    s.chars().map(|c| { let v = c as u32; if v < 256 { v as u8 } else { b'?' } }).collect()
}
/// Hard-clip a string to a max width (no ellipsis char, which avoids the
/// WinAnsi fallback turning an ellipsis into '?').
fn fit(s: &str, bold: bool, size: f32, max_w: f32) -> String {
    if width_of(s, bold, size) <= max_w { return s.to_string(); }
    let mut out = s.to_string();
    while !out.is_empty() && width_of(&out, bold, size) > max_w { out.pop(); }
    out
}
fn wrap(s: &str, bold: bool, size: f32, max_w: f32) -> Vec<String> {
    let s = s.replace(['\r', '\n'], " ");
    let s = s.trim();
    if s.is_empty() { return Vec::new(); }
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let test = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if width_of(&test, bold, size) > max_w && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = test;
        }
    }
    if !cur.is_empty() { lines.push(cur); }
    lines
}

// ---------- coordinate helpers ----------
fn col_x(c: i64) -> f32 { MARGIN_X + (c - 1) as f32 * COL_W } // left edge of column c (1-indexed)
/// page index (0-based) and the y of the top of row r (pdf bottom-origin coords)
fn row_page_top(r: i64) -> (i64, f32) {
    let page = (r - 1) / ROWS_PER_PAGE;
    let local_top_offset = ((r - 1) % ROWS_PER_PAGE) as f32 * ROW_H;
    (page, FORM_TOP - local_top_offset)
}

// ---------- renderer ----------
struct Renderer {
    pages: Vec<Content>,
    npages: usize,
}
impl Renderer {
    fn new(npages: usize) -> Self {
        let mut pages = Vec::with_capacity(npages);
        for _ in 0..npages { pages.push(Content::new()); }
        Renderer { pages, npages }
    }
    fn page(&mut self, p: i64) -> &mut Content {
        &mut self.pages[p as usize]
    }
    fn line(&mut self, p: i64, x1: f32, y1: f32, x2: f32, y2: f32, w: f32) {
        let c = self.page(p);
        c.set_stroke_rgb(0.0, 0.0, 0.0);
        c.set_line_width(w);
        c.move_to(x1, y1);
        c.line_to(x2, y2);
        c.stroke();
    }
    fn text(&mut self, p: i64, s: &str, x: f32, baseline: f32, bold: bool, size: f32, col: (f32, f32, f32)) {
        if s.is_empty() { return; }
        let font = if bold { F_BOLD } else { F_REG };
        let c = self.page(p);
        c.set_fill_rgb(col.0, col.1, col.2);
        c.begin_text();
        c.set_font(font, size);
        c.set_text_matrix([1.0, 0.0, 0.0, 1.0, x, baseline]);
        c.show(Str(&winansi(s)));
        c.end_text();
    }
    fn checkbox(&mut self, p: i64, cx: f32, cy: f32, sz: f32, checked: bool) {
        let x = cx - sz / 2.0;
        let y = cy - sz / 2.0;
        {
            let c = self.page(p);
            c.set_fill_rgb(WHITE.0, WHITE.1, WHITE.2);
            c.rect(x, y, sz, sz);
            c.fill_nonzero();
            c.set_stroke_rgb(0.25, 0.25, 0.25);
            c.set_line_width(0.7);
            c.rect(x, y, sz, sz);
            c.stroke();
        }
        if checked {
            let c = self.page(p);
            c.set_stroke_rgb(0.043, 0.43, 0.31);
            c.set_line_width(1.2);
            c.move_to(x + sz * 0.17, y + sz * 0.45);
            c.line_to(x + sz * 0.40, y + sz * 0.20);
            c.line_to(x + sz * 0.84, y + sz * 0.80);
            c.stroke();
        }
    }
}

/// Build the full report PDF and return its bytes.
pub fn build_pdf(data: &ReportData, logo: Option<&Logo>) -> Vec<u8> {
    let tpl = parse_template();
    // total rows incl. signatures
    let max_row = 74i64;
    let npages = (((max_row - 1) / ROWS_PER_PAGE) + 1) as usize;
    let mut r = Renderer::new(npages);

    // merge lookup: (r,c) top-left -> (r1,c1)
    let mut merge_span: BTreeMap<(i64, i64), (i64, i64)> = BTreeMap::new();
    for m in &tpl.merges {
        merge_span.insert((m.r0, m.c0), (m.r1, m.c1));
    }
    // columns (per row) that carry a real label, so labels can overflow into
    // genuinely empty neighbour cells like Excel does, but stop at the next label.
    let mut label_cols: BTreeSet<(i64, i64)> = BTreeSet::new();
    for t in &tpl.texts {
        if !t.text.eq_ignore_ascii_case("false") && !t.text.eq_ignore_ascii_case("true") {
            label_cols.insert((t.r, t.c));
        }
    }

    // ----- borders (the boxes / underlines of the official form) -----
    for b in &tpl.borders {
        let (p, top) = row_page_top(b.r);
        let x0 = col_x(b.c);
        let x1 = col_x(b.c + 1);
        let bottom = top - ROW_H;
        let lw = |w: u8| if w == 2 { 1.1 } else { 0.5 };
        if b.t > 0 { r.line(p, x0, top, x1, top, lw(b.t)); }
        if b.b > 0 { r.line(p, x0, bottom, x1, bottom, lw(b.b)); }
        if b.l > 0 { r.line(p, x0, top, x0, bottom, lw(b.l)); }
        if b.rt > 0 { r.line(p, x1, top, x1, bottom, lw(b.rt)); }
    }

    // ----- template label text -----
    for t in &tpl.texts {
        // skip the False/True booleans that excel stores for checkboxes
        if t.text.eq_ignore_ascii_case("false") || t.text.eq_ignore_ascii_case("true") { continue; }
        let (p, top) = row_page_top(t.r);
        // determine cell/merge rectangle
        let (r1, c1) = merge_span.get(&(t.r, t.c)).copied().unwrap_or((t.r, t.c));
        let x0 = col_x(t.c);
        // allow LEFT-aligned, non-merged labels to overflow rightwards into
        // empty cells, stopping at the next label (Excel overflow model).
        // Centered/right text keeps its true cell width so it stays put.
        let mut eff_c1 = c1;
        if c1 == t.c && t.h == 0 {
            let mut cc = t.c + 1;
            while cc <= COLS && !label_cols.contains(&(t.r, cc)) { cc += 1; }
            eff_c1 = cc - 1;
        }
        let x_right = col_x(eff_c1 + 1);
        let cell_w = x_right - x0;
        let rows_tall = (r1 - t.r + 1) as f32;
        let bottom = top - ROW_H * rows_tall;
        let size = t.size * FONT_SCALE;
        let col = if t.text == "City of Edmonton" { EDM_BLUE } else { BLACK };
        let blue_big = t.text == "City of Edmonton";
        // handle multi-line label (Overtime \nJustification:)
        let raw_lines: Vec<&str> = t.text.split('\n').collect();
        let line_h = size * 1.15;
        // vertical: section headers (center merges) center; else bottom-aligned
        let centered_v = r1 > t.r;
        let total_h = raw_lines.len() as f32 * line_h;
        let mut ly = if centered_v {
            (top + bottom) / 2.0 + total_h / 2.0 - size
        } else {
            top - ROW_H + (ROW_H - size) * 0.5 + size * 0.08 // bottom-ish within first row
        };
        for ln in raw_lines {
            let fitted = fit(ln, t.bold || blue_big, size, cell_w - 3.0);
            let tx = match t.h {
                1 => x0 + (cell_w - width_of(&fitted, t.bold || blue_big, size)) / 2.0,
                2 => x_right - 3.0 - width_of(&fitted, t.bold || blue_big, size),
                _ => x0 + 2.0,
            };
            r.text(p, &fitted, tx, ly, t.bold || blue_big, size, col);
            ly -= line_h;
        }
    }

    // ----- user data overlay -----
    let body_size = 10.0 * FONT_SCALE;
    for (name, row, c0, c1) in field_cells() {
        let val = data.t(name);
        if val.is_empty() { continue; }
        let (p, top) = row_page_top(row);
        let x0 = col_x(c0) + 2.0;
        let x_right = col_x(c1 + 1);
        let cell_w = x_right - x0 - 2.0;
        // WIP descriptions can wrap within the row band (single row, but allow shrink-to-fit)
        let baseline = top - ROW_H + (ROW_H - body_size) * 0.5 + body_size * 0.10;
        let fitted = fit(val, false, body_size, cell_w);
        r.text(p, &fitted, x0, baseline, false, body_size, BLACK);
    }

    // overtime + work-outside are multi-row text areas: flow wrapped lines
    flow_multirow(&mut r, data.t("overtime"), 12, 13, 2, 11, body_size);
    flow_multirow(&mut r, data.t("workOutside"), 44, 51, 1, 10, body_size);

    // ----- checkboxes -----
    let cb_sz = (ROW_H - 3.0).min(8.5);
    for (name, row, col) in checkbox_cells() {
        let (p, top) = row_page_top(row);
        let cx = col_x(col) + COL_W / 2.0;
        let cy = top - ROW_H / 2.0;
        r.checkbox(p, cx, cy, cb_sz, data.c(name));
    }

    // ----- logo (page 1, top-left) -----
    let has_logo = logo.is_some();
    if has_logo {
        // place at rows 1..4 (cols A..B): a ~square in the top-left
        let ls = (ROW_H * 4.0).min(COL_W * 2.0);
        let lx = MARGIN_X + 1.0;
        let ly = FORM_TOP - ls - 1.0;
        let c = r.page(0);
        c.save_state();
        c.transform([ls, 0.0, 0.0, ls, lx, ly]);
        c.x_object(Name(b"Logo"));
        c.restore_state();
    }

    assemble(r.pages, logo, has_logo, r.npages)
}

/// Flow wrapped text across a band of sheet rows (row_top..=row_bottom).
fn flow_multirow(r: &mut Renderer, text: &str, row_top: i64, row_bottom: i64, c0: i64, c1: i64, size: f32) {
    if text.trim().is_empty() { return; }
    let x0 = col_x(c0) + 3.0;
    let x_right = col_x(c1 + 1);
    let lines = wrap(text, false, size, x_right - x0 - 4.0);
    let mut row = row_top;
    for ln in lines {
        if row > row_bottom { break; }
        let (p, top) = row_page_top(row);
        let baseline = top - ROW_H + (ROW_H - size) * 0.5 + size * 0.10;
        r.text(p, &ln, x0, baseline, false, size, BLACK);
        row += 1;
    }
}

fn assemble(pages: Vec<Content>, logo: Option<&Logo>, has_logo: bool, npages: usize) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog = Ref::new(1);
    let tree = Ref::new(2);
    let font_reg = Ref::new(3);
    let font_bold = Ref::new(4);
    let logo_ref = Ref::new(5);
    let mut next = 6i32;
    let mut alloc = || { let r = Ref::new(next); next += 1; r };
    let page_refs: Vec<Ref> = (0..npages).map(|_| alloc()).collect();
    let content_refs: Vec<Ref> = (0..npages).map(|_| alloc()).collect();

    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree).kids(page_refs.iter().copied()).count(npages as i32);
    pdf.type1_font(font_reg).base_font(Name(b"Helvetica")).encoding_predefined(Name(b"WinAnsiEncoding"));
    pdf.type1_font(font_bold).base_font(Name(b"Helvetica-Bold")).encoding_predefined(Name(b"WinAnsiEncoding"));
    if let Some(l) = logo {
        let mut img = pdf.image_xobject(logo_ref, l.jpeg);
        img.filter(Filter::DctDecode);
        img.width(l.w);
        img.height(l.h);
        img.color_space().device_rgb();
        img.bits_per_component(8);
    }
    for (i, content) in pages.into_iter().enumerate() {
        {
            let mut page = pdf.page(page_refs[i]);
            page.parent(tree);
            page.media_box(Rect::new(0.0, 0.0, PAGE_W, PAGE_H));
            page.contents(content_refs[i]);
            let mut res = page.resources();
            { let mut fonts = res.fonts(); fonts.pair(F_REG, font_reg); fonts.pair(F_BOLD, font_bold); }
            if has_logo && i == 0 { res.x_objects().pair(Name(b"Logo"), logo_ref); }
        }
        pdf.stream(content_refs[i], &content.finish());
    }
    pdf.finish().to_vec()
}

// ---------- data marker (re-import) ----------
pub const MARKER: &str = "%%DCPR-DATA:";
pub fn append_data_marker(pdf: &mut Vec<u8>, data: &ReportData) {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let b64 = STANDARD.encode(to_json(data).as_bytes());
    pdf.extend_from_slice(b"\n");
    pdf.extend_from_slice(MARKER.as_bytes());
    pdf.extend_from_slice(b64.as_bytes());
    pdf.extend_from_slice(b"\n");
}
pub fn extract_data_marker(bytes: &[u8]) -> Option<ReportData> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let needle = MARKER.as_bytes();
    let pos = bytes.windows(needle.len()).rposition(|w| w == needle)?;
    let rest = &bytes[pos + needle.len()..];
    let end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
    let json = STANDARD.decode(&rest[..end]).ok()?;
    let s = String::from_utf8(json).ok()?;
    Some(from_json(&s))
}

pub fn to_json(data: &ReportData) -> String {
    let mut map = serde_json::Map::new();
    for (k, v) in &data.text { map.insert(k.clone(), Value::String(v.clone())); }
    for k in &data.checks { map.insert(k.clone(), Value::Bool(true)); }
    Value::Object(map).to_string()
}
pub fn from_json(s: &str) -> ReportData {
    let mut data = ReportData::default();
    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(s) {
        for (k, v) in map {
            match v {
                Value::String(s) => { data.text.insert(k, s); }
                Value::Bool(true) => { data.checks.insert(k); }
                _ => {}
            }
        }
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_sample_pdf() {
        let json = std::fs::read_to_string("tests/sample_data.json").expect("sample json");
        let data = from_json(&json);
        let jpeg = std::fs::read("assets/logo.jpg").expect("logo");
        let logo = Logo { jpeg: &jpeg, w: 250, h: 250 };
        let bytes = build_pdf(&data, Some(&logo));
        assert!(bytes.starts_with(b"%PDF"));
        std::fs::write("tests/out.pdf", &bytes).unwrap();
    }
    #[test]
    fn marker_round_trips() {
        let data = from_json(&std::fs::read_to_string("tests/sample_data.json").unwrap());
        let mut bytes = build_pdf(&data, None);
        append_data_marker(&mut bytes, &data);
        let back = extract_data_marker(&bytes).unwrap();
        assert_eq!(back.text.get("location"), data.text.get("location"));
        assert_eq!(back.c("barr_arrival"), data.c("barr_arrival"));
    }
}
