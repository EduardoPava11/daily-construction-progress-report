//! Pure-Rust vector PDF renderer. Reproduces the City of Edmonton Daily
//! Construction Progress Report by replaying the ORIGINAL spreadsheet geometry
//! (cell borders, merges, and label text extracted from the .xlsx) and
//! overlaying the user's data into the exact input cells.
//!
//! Rows have VARIABLE height: any cell whose content wraps to several lines
//! grows its row (and everything below flows down), like auto-fit in Excel.
//! No web-sys here, so it is host-testable on the host with `cargo test`.

use std::collections::{BTreeMap, BTreeSet};

use pdf_writer::{Content, Filter, Name, Pdf, Rect, Ref, Str};
use serde_json::Value;

use crate::metrics::{HELVETICA, HELVETICA_BOLD};

// ---------- page + grid geometry (calibrated to the fit-to-width original) --
const PAGE_W: f32 = 612.0;
const PAGE_H: f32 = 792.0;
const MARGIN_X: f32 = 36.0;
const MARGIN_Y: f32 = 24.0;
const COLS: i64 = 12;
const MAX_ROW: i64 = 74;
const COL_W: f32 = (PAGE_W - 2.0 * MARGIN_X) / COLS as f32; // 45
const ROW_H: f32 = 11.1; // default (single-line) row height
const FONT_SCALE: f32 = 0.70; // Arial pt -> rendered pt at fit-to-width scale
const BODY: f32 = 10.0 * FONT_SCALE; // 7pt body text
const LINE_H: f32 = 8.0; // line advance for wrapped multi-line cell text
const FORM_TOP: f32 = PAGE_H - MARGIN_Y;

// ---------- colors ----------
const BLACK: (f32, f32, f32) = (0.0, 0.0, 0.0);
const EDM_BLUE: (f32, f32, f32) = (0.0, 0.18, 0.46);
const WHITE: (f32, f32, f32) = (1.0, 1.0, 1.0);

const F_REG: Name = Name(b"F1");
const F_BOLD: Name = Name(b"F2");

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
struct Txt { r: i64, c: i64, text: String, size: f32, bold: bool, h: u8 }
struct Template { merges: Vec<Merge>, borders: Vec<Border>, texts: Vec<Txt> }

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
        let halign = match t["h"].as_str().unwrap_or("") { "center" => 1u8, "right" => 2u8, _ => 0u8 };
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

fn leak(s: String) -> &'static str { &*Box::leak(s.into_boxed_str()) }

// ---------- field overlay map: name -> (row, col_start, col_end) ----------
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
    for col in 0..5i64 {
        for row in 0..8i64 {
            let crew = leak(format!("wf_{col}_{row}_crew"));
            let equip = leak(format!("wf_{col}_{row}_equip"));
            let base = 1 + col * 2;
            v.push((crew, 23 + row, base, base));
            v.push((equip, 23 + row, base + 1, base + 1));
        }
    }
    for i in 0..8i64 {
        let n = leak(format!("wip_{i}_text"));
        v.push((n, 33 + i, 1, 9));
    }
    v
}

fn checkbox_cells() -> Vec<(&'static str, i64, i64)> {
    let mut v = Vec::new();
    for i in 0..8i64 {
        let n = leak(format!("wip_{i}_lab"));
        v.push((n, 33 + i, 10));
    }
    let left = ["barr_arrival","barr_departure","barr_secure","exc_truckbox","fill_lift","sub_elev","cb_grade","cb_elev","cb_thick","conc_straight","conc_elev","conc_cross","conc_forms","sc_grade","sc_lift"];
    for (i, name) in left.iter().enumerate() { v.push((*name, 55 + i as i64, 5)); }
    let right = ["dr_pipe","dr_grade","dr_elev","ls_topsoil","pv_grade","pv_straight","pv_joints","as_notify","as_lift","as_spread","fdr_grade","fdr_cement","fdr_visual"];
    for (i, name) in right.iter().enumerate() { v.push((*name, 55 + i as i64, 9)); }
    v
}

// work-outside contract free-text region (rows inclusive) and its columns
const WO_ROW0: i64 = 44;
const WO_ROW1: i64 = 51;
const WO_C0: i64 = 1;
const WO_C1: i64 = 10;
// overtime justification input
const OT_ROW: i64 = 12;
const OT_C0: i64 = 2;
const OT_C1: i64 = 11;

// ---------- text metrics ----------
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

fn col_x(c: i64) -> f32 { MARGIN_X + (c - 1) as f32 * COL_W }
/// height needed to hold `lines` lines of text in a cell
fn lines_height(lines: usize) -> f32 {
    if lines <= 1 { ROW_H } else { ROW_H + (lines as f32 - 1.0) * LINE_H }
}

// ---------- renderer ----------
struct Renderer { pages: Vec<Content> }
impl Renderer {
    fn new(n: usize) -> Self {
        let mut pages = Vec::with_capacity(n);
        for _ in 0..n { pages.push(Content::new()); }
        Renderer { pages }
    }
    fn p(&mut self, p: i64) -> &mut Content { &mut self.pages[p as usize] }
    fn line(&mut self, p: i64, x1: f32, y1: f32, x2: f32, y2: f32, w: f32) {
        let c = self.p(p);
        c.set_stroke_rgb(0.0, 0.0, 0.0);
        c.set_line_width(w);
        c.move_to(x1, y1);
        c.line_to(x2, y2);
        c.stroke();
    }
    fn text(&mut self, p: i64, s: &str, x: f32, baseline: f32, bold: bool, size: f32, col: (f32, f32, f32)) {
        if s.is_empty() { return; }
        let font = if bold { F_BOLD } else { F_REG };
        let c = self.p(p);
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
            let c = self.p(p);
            c.set_fill_rgb(WHITE.0, WHITE.1, WHITE.2);
            c.rect(x, y, sz, sz);
            c.fill_nonzero();
            c.set_stroke_rgb(0.25, 0.25, 0.25);
            c.set_line_width(0.7);
            c.rect(x, y, sz, sz);
            c.stroke();
        }
        if checked {
            let c = self.p(p);
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

    // merge lookups
    let mut merge_top: BTreeMap<(i64, i64), (i64, i64)> = BTreeMap::new(); // (r0,c0)->(r1,c1)
    let mut covered: BTreeMap<(i64, i64), (i64, i64, i64, i64)> = BTreeMap::new();
    for m in &tpl.merges {
        merge_top.insert((m.r0, m.c0), (m.r1, m.c1));
        for r in m.r0..=m.r1 {
            for c in m.c0..=m.c1 {
                covered.insert((r, c), (m.r0, m.c0, m.r1, m.c1));
            }
        }
    }

    // ----- 1. compute per-row line counts (-> variable heights) -----
    let mut row_lines = vec![1usize; (MAX_ROW + 2) as usize];
    let set_min = |rl: &mut Vec<usize>, r: i64, n: usize| {
        let e = &mut rl[r as usize];
        if n > *e { *e = n; }
    };
    // template labels with explicit newlines (e.g. "Overtime \nJustification:")
    for t in &tpl.texts {
        if t.text.eq_ignore_ascii_case("false") || t.text.eq_ignore_ascii_case("true") { continue; }
        // only single-row cells force height; merged multi-row titles have room
        let single_row = match covered.get(&(t.r, t.c)) {
            Some(&(r0, _, r1, _)) => r0 == r1,
            None => true,
        };
        if single_row {
            let n = t.text.split('\n').count();
            if n > 1 { set_min(&mut row_lines, t.r, n); }
        }
    }
    // user single-row fields
    for (name, row, c0, c1) in field_cells() {
        let val = data.t(name);
        if val.is_empty() { continue; }
        let w = col_x(c1 + 1) - col_x(c0) - 4.0;
        let n = wrap(val, false, BODY, w).len().max(1);
        set_min(&mut row_lines, row, n);
    }
    // overtime (single row, grows)
    {
        let val = data.t("overtime");
        if !val.is_empty() {
            let w = col_x(OT_C1 + 1) - col_x(OT_C0) - 4.0;
            let n = wrap(val, false, BODY, w).len().max(1);
            set_min(&mut row_lines, OT_ROW, n);
        }
    }
    // work-outside: flow across its 8 rows, growing them uniformly if needed
    let wo_lines: Vec<String> = {
        let w = col_x(WO_C1 + 1) - col_x(WO_C0) - 6.0;
        wrap(data.t("workOutside"), false, BODY, w)
    };
    let wo_rows = (WO_ROW1 - WO_ROW0 + 1) as usize;
    let wo_per_row = ((wo_lines.len() + wo_rows - 1) / wo_rows).max(1);
    if wo_lines.len() > wo_rows {
        for r in WO_ROW0..=WO_ROW1 { set_min(&mut row_lines, r, wo_per_row); }
    }

    // row heights
    let mut row_h = vec![ROW_H; (MAX_ROW + 2) as usize];
    for r in 1..=MAX_ROW { row_h[r as usize] = lines_height(row_lines[r as usize]); }

    // ----- 2. position rows with pagination that keeps merged blocks intact -----
    // a break may start before row r unless r is inside a vertical merge
    let mut break_ok = vec![true; (MAX_ROW + 2) as usize];
    for m in &tpl.merges {
        for r in (m.r0 + 1)..=m.r1 { break_ok[r as usize] = false; }
    }
    let mut page_of = vec![0i64; (MAX_ROW + 2) as usize];
    let mut top_of = vec![0f32; (MAX_ROW + 2) as usize];
    let mut page = 0i64;
    let mut cursor = FORM_TOP;
    let mut r = 1i64;
    while r <= MAX_ROW {
        if break_ok[r as usize] {
            // block = r .. next break_ok - 1
            let mut e = r + 1;
            while e <= MAX_ROW && !break_ok[e as usize] { e += 1; }
            let block_h: f32 = (r..e).map(|x| row_h[x as usize]).sum();
            if cursor - block_h < MARGIN_Y && cursor < FORM_TOP - 0.1 {
                page += 1;
                cursor = FORM_TOP;
            }
        }
        page_of[r as usize] = page;
        top_of[r as usize] = cursor;
        cursor -= row_h[r as usize];
        r += 1;
    }
    let npages = (page + 1) as usize;
    let mut rr = Renderer::new(npages);

    // helpers closing over geometry
    let cell_height = |r0: i64, r1: i64| -> f32 { (r0..=r1).map(|x| row_h[x as usize]).sum() };

    // ----- 3. borders (perimeter-only for merged cells) -----
    for b in &tpl.borders {
        let p = page_of[b.r as usize];
        let top = top_of[b.r as usize];
        let bottom = top - row_h[b.r as usize];
        let x0 = col_x(b.c);
        let x1 = col_x(b.c + 1);
        // suppress edges interior to a merge
        let (mr0, mc0, mr1, mc1) = covered.get(&(b.r, b.c)).copied().unwrap_or((b.r, b.c, b.r, b.c));
        let draw_t = b.t > 0 && b.r == mr0;
        let draw_b = b.b > 0 && b.r == mr1;
        let draw_l = b.l > 0 && b.c == mc0;
        let draw_r = b.rt > 0 && b.c == mc1;
        let lw = |w: u8| if w == 2 { 1.1 } else { 0.5 };
        if draw_t { rr.line(p, x0, top, x1, top, lw(b.t)); }
        if draw_b { rr.line(p, x0, bottom, x1, bottom, lw(b.b)); }
        if draw_l { rr.line(p, x0, top, x0, bottom, lw(b.l)); }
        if draw_r { rr.line(p, x1, top, x1, bottom, lw(b.rt)); }
    }

    // ----- 4. template label text -----
    let mut label_cols: BTreeSet<(i64, i64)> = BTreeSet::new();
    for t in &tpl.texts {
        if !t.text.eq_ignore_ascii_case("false") && !t.text.eq_ignore_ascii_case("true") {
            label_cols.insert((t.r, t.c));
        }
    }
    for t in &tpl.texts {
        if t.text.eq_ignore_ascii_case("false") || t.text.eq_ignore_ascii_case("true") { continue; }
        let p = page_of[t.r as usize];
        let top = top_of[t.r as usize];
        let (r1, c1) = merge_top.get(&(t.r, t.c)).copied().unwrap_or((t.r, t.c));
        let x0 = col_x(t.c);
        let mut eff_c1 = c1;
        if c1 == t.c && t.h == 0 {
            let mut cc = t.c + 1;
            while cc <= COLS && !label_cols.contains(&(t.r, cc)) { cc += 1; }
            eff_c1 = cc - 1;
        }
        let x_right = col_x(eff_c1 + 1);
        let cell_w = x_right - x0;
        let h = cell_height(t.r, r1);
        let bottom = top - h;
        let size = t.size * FONT_SCALE;
        let blue = t.text == "City of Edmonton";
        let bold = t.bold || blue;
        let col = if blue { EDM_BLUE } else { BLACK };
        let raw_lines: Vec<&str> = t.text.split('\n').collect();
        let lh = size * 1.15;
        let centered_v = r1 > t.r;
        let total_h = raw_lines.len() as f32 * lh;
        let mut ly = if centered_v {
            (top + bottom) / 2.0 + total_h / 2.0 - size
        } else {
            top - ROW_H + (ROW_H - size) * 0.5 + size * 0.08
        };
        for ln in raw_lines {
            let fitted = fit(ln, bold, size, cell_w - 3.0);
            let tx = match t.h {
                1 => x0 + (cell_w - width_of(&fitted, bold, size)) / 2.0,
                2 => x_right - 3.0 - width_of(&fitted, bold, size),
                _ => x0 + 2.0,
            };
            rr.text(p, &fitted, tx, ly, bold, size, col);
            ly -= lh;
        }
    }

    // ----- 5. user data overlay (wraps + uses the grown row height) -----
    let draw_wrapped = |rr: &mut Renderer, p: i64, top: f32, x: f32, w: f32, lines: &[String]| {
        let mut ly = top - LINE_H * 0.9;
        for ln in lines {
            rr.text(p, ln, x, ly, false, BODY, BLACK);
            ly -= LINE_H;
        }
        let _ = w;
    };
    for (name, row, c0, c1) in field_cells() {
        let val = data.t(name);
        if val.is_empty() { continue; }
        let p = page_of[row as usize];
        let top = top_of[row as usize];
        let x0 = col_x(c0) + 2.0;
        let w = col_x(c1 + 1) - x0 - 2.0;
        let lines = wrap(val, false, BODY, w);
        draw_wrapped(&mut rr, p, top, x0, w, &lines);
    }
    // overtime
    {
        let val = data.t("overtime");
        if !val.is_empty() {
            let p = page_of[OT_ROW as usize];
            let top = top_of[OT_ROW as usize];
            let x0 = col_x(OT_C0) + 2.0;
            let w = col_x(OT_C1 + 1) - x0 - 2.0;
            let lines = wrap(val, false, BODY, w);
            draw_wrapped(&mut rr, p, top, x0, w, &lines);
        }
    }
    // work-outside: chunk lines across its rows
    if !wo_lines.is_empty() {
        let x0 = col_x(WO_C0) + 3.0;
        let mut idx = 0usize;
        for row in WO_ROW0..=WO_ROW1 {
            if idx >= wo_lines.len() { break; }
            let p = page_of[row as usize];
            let top = top_of[row as usize];
            let take = wo_per_row.min(wo_lines.len() - idx);
            let chunk = &wo_lines[idx..idx + take];
            draw_wrapped(&mut rr, p, top, x0, 0.0, chunk);
            idx += take;
        }
    }

    // ----- 6. checkboxes -----
    let cb_sz = (ROW_H - 3.0).min(8.5);
    for (name, row, col) in checkbox_cells() {
        let p = page_of[row as usize];
        let top = top_of[row as usize];
        let cx = col_x(col) + COL_W / 2.0;
        let cy = top - row_h[row as usize] / 2.0;
        rr.checkbox(p, cx, cy, cb_sz, data.c(name));
    }

    // ----- 7. logo (page 1, top-left) -----
    let has_logo = logo.is_some();
    if has_logo {
        let ls = (ROW_H * 4.0).min(COL_W * 2.0);
        let lx = MARGIN_X + 1.0;
        let ly = FORM_TOP - ls - 1.0;
        let c = rr.p(0);
        c.save_state();
        c.transform([ls, 0.0, 0.0, ls, lx, ly]);
        c.x_object(Name(b"Logo"));
        c.restore_state();
    }

    assemble(rr.pages, logo, has_logo, npages)
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
    fn grows_rows_for_long_content() {
        // a giant WIP description + work-outside should add pages, not clip
        let mut data = from_json(&std::fs::read_to_string("tests/sample_data.json").unwrap());
        let long = "Lorem ipsum dolor sit amet ".repeat(40);
        data.text.insert("wip_2_text".into(), long.clone());
        data.text.insert("workOutside".into(), long);
        let bytes = build_pdf(&data, None);
        std::fs::write("tests/out_grow.pdf", &bytes).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
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
