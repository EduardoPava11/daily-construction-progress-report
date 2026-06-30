//! Pure-Rust vector PDF renderer for the Daily Construction Progress Report.
//! No web-sys here, so it can be unit-tested on the host with `cargo test`.

use std::collections::{BTreeMap, BTreeSet};

use pdf_writer::{Content, Filter, Name, Pdf, Rect, Ref, Str};

use crate::metrics::{HELVETICA, HELVETICA_BOLD};
use crate::model::{CK_LEFT, CK_RIGHT, WF_COLS, WF_DEFAULT_NAMES, WF_ROWS, WIP_LEGEND, WIP_ROWS};

// ---- page geometry (US Letter, points) ----
const PAGE_W: f32 = 612.0;
const PAGE_H: f32 = 792.0;
const MARGIN: f32 = 36.0;
const CONTENT_W: f32 = PAGE_W - MARGIN * 2.0; // 540
const LEFT: f32 = MARGIN;
const RIGHT: f32 = PAGE_W - MARGIN;

// ---- colors (r,g,b in 0..1) ----
const NAVY: (f32, f32, f32) = (0.122, 0.231, 0.341);
const GRAYHDR: (f32, f32, f32) = (0.93, 0.95, 0.96);
const BORDER: (f32, f32, f32) = (0.62, 0.65, 0.69);
const INK: (f32, f32, f32) = (0.10, 0.10, 0.12);
const LABEL: (f32, f32, f32) = (0.20, 0.26, 0.31);
const WHITE: (f32, f32, f32) = (1.0, 1.0, 1.0);
const GREEN: (f32, f32, f32) = (0.043, 0.431, 0.31);
const SUBTLE: (f32, f32, f32) = (0.35, 0.35, 0.38);

const F_REG: Name = Name(b"F1");
const F_BOLD: Name = Name(b"F2");

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

/// A JPEG logo plus its pixel dimensions, embedded via DCTDecode.
pub struct Logo<'a> {
    pub jpeg: &'a [u8],
    pub w: i32,
    pub h: i32,
}

// ---- text metrics ----
fn width_of(s: &str, bold: bool, size: f32) -> f32 {
    let table = if bold { &HELVETICA_BOLD } else { &HELVETICA };
    let mut w: u32 = 0;
    for ch in s.chars() {
        let c = ch as usize;
        let adv = if c < 256 { table[c] } else { table[b'?' as usize] };
        w += adv as u32;
    }
    (w as f32) / 1000.0 * size
}

/// Encode a string into WinAnsi/Latin-1 bytes for a base-14 font Str.
fn winansi(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| {
            let v = c as u32;
            if v < 256 {
                v as u8
            } else {
                b'?'
            }
        })
        .collect()
}

fn fit(s: &str, bold: bool, size: f32, max_w: f32) -> String {
    if width_of(s, bold, size) <= max_w {
        return s.to_string();
    }
    let mut out: String = s.to_string();
    while !out.is_empty() && width_of(&format!("{out}\u{2026}"), bold, size) > max_w {
        out.pop();
    }
    format!("{out}\u{2026}")
}

fn wrap(s: &str, bold: bool, size: f32, max_w: f32) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let test = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if width_of(&test, bold, size) > max_w {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            if width_of(word, bold, size) > max_w {
                // break an over-long word by characters
                let mut piece = String::new();
                for ch in word.chars() {
                    if width_of(&format!("{piece}{ch}"), bold, size) > max_w && !piece.is_empty() {
                        lines.push(std::mem::take(&mut piece));
                    }
                    piece.push(ch);
                }
                cur = piece;
            } else {
                cur = word.to_string();
            }
        } else {
            cur = test;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

// ---- renderer ----
struct Renderer {
    pages: Vec<Content>,
    cur: Content,
    y: f32,
    has_logo: bool,
}

impl Renderer {
    fn new(has_logo: bool) -> Self {
        Renderer {
            pages: Vec::new(),
            cur: Content::new(),
            y: PAGE_H - MARGIN,
            has_logo,
        }
    }

    fn new_page(&mut self) {
        let done = std::mem::replace(&mut self.cur, Content::new());
        self.pages.push(done);
        self.y = PAGE_H - MARGIN;
    }

    /// Emit the logo image draw op into the current content stream. The
    /// image occupies a 1x1 unit square, so we scale + translate it via CTM.
    fn draw_logo(&mut self) {
        let ls = 46.0;
        self.cur.save_state();
        self.cur.transform([ls, 0.0, 0.0, ls, LEFT, PAGE_H - MARGIN - ls]);
        self.cur.x_object(Name(b"Logo"));
        self.cur.restore_state();
    }
    fn space(&self) -> f32 {
        self.y - MARGIN
    }
    fn ensure(&mut self, h: f32) {
        if self.y - h < MARGIN {
            self.new_page();
        }
    }

    fn fill_rect(&mut self, x: f32, top: f32, w: f32, h: f32, c: (f32, f32, f32)) {
        self.cur.set_fill_rgb(c.0, c.1, c.2);
        self.cur.rect(x, top - h, w, h);
        self.cur.fill_nonzero();
    }
    fn stroke_rect(&mut self, x: f32, top: f32, w: f32, h: f32, c: (f32, f32, f32), lw: f32) {
        self.cur.set_stroke_rgb(c.0, c.1, c.2);
        self.cur.set_line_width(lw);
        self.cur.rect(x, top - h, w, h);
        self.cur.stroke();
    }
    /// White-filled, gray-bordered box.
    fn box_(&mut self, x: f32, top: f32, w: f32, h: f32) {
        self.fill_rect(x, top, w, h, WHITE);
        self.stroke_rect(x, top, w, h, BORDER, 0.75);
    }

    fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, c: (f32, f32, f32), lw: f32) {
        self.cur.set_stroke_rgb(c.0, c.1, c.2);
        self.cur.set_line_width(lw);
        self.cur.move_to(x1, y1);
        self.cur.line_to(x2, y2);
        self.cur.stroke();
    }

    fn text(&mut self, s: &str, x: f32, baseline: f32, bold: bool, size: f32, c: (f32, f32, f32)) {
        if s.is_empty() {
            return;
        }
        let font = if bold { F_BOLD } else { F_REG };
        self.cur.set_fill_rgb(c.0, c.1, c.2);
        self.cur.begin_text();
        self.cur.set_font(font, size);
        self.cur.set_text_matrix([1.0, 0.0, 0.0, 1.0, x, baseline]);
        self.cur.show(Str(&winansi(s)));
        self.cur.end_text();
    }

    /// vertically-centered text in a box; hcenter optionally horizontally centers
    fn vtext(&mut self, s: &str, x: f32, top: f32, w: f32, h: f32, bold: bool, size: f32, c: (f32, f32, f32), hcenter: bool) {
        if s.is_empty() {
            return;
        }
        let baseline = top - h / 2.0 - size * 0.35;
        let tx = if hcenter {
            x + (w - width_of(s, bold, size)) / 2.0
        } else {
            x + 3.0
        };
        self.text(s, tx, baseline, bold, size, c);
    }

    fn checkbox(&mut self, x: f32, top: f32, sz: f32, checked: bool) {
        self.box_(x, top, sz, sz);
        if checked {
            let b = top - sz;
            self.line(x + sz * 0.17, b + sz * 0.45, x + sz * 0.40, b + sz * 0.20, GREEN, 1.4);
            self.line(x + sz * 0.40, b + sz * 0.20, x + sz * 0.83, b + sz * 0.78, GREEN, 1.4);
        }
    }

    fn band(&mut self, title: &str, right: Option<&str>) {
        self.ensure(24.0);
        let h = 18.0;
        self.fill_rect(LEFT, self.y, CONTENT_W, h, NAVY);
        self.text(title, LEFT + 8.0, self.y - 13.0, true, 10.0, WHITE);
        if let Some(r) = right {
            let w = width_of(r, true, 9.0);
            self.text(r, RIGHT - 8.0 - w, self.y - 12.5, true, 9.0, WHITE);
        }
        self.y -= h + 6.0;
    }

    /// Inline labeled field: bold label, then a boxed value, both on one line.
    fn field(&mut self, x: f32, w: f32, label: &str, value: &str) {
        let h = 18.0;
        let lw = width_of(label, true, 8.0) + 5.0;
        self.text(label, x, self.y - h / 2.0 - 8.0 * 0.35, true, 8.0, LABEL);
        let bx = x + lw;
        let bw = w - lw;
        self.box_(bx, self.y, bw, h);
        let v = fit(value, false, 9.0, bw - 8.0);
        self.vtext(&v, bx + 1.0, self.y, bw, h, false, 9.0, INK, false);
    }

    /// Label above a multi-line box.
    fn field_multi(&mut self, x: f32, w: f32, label: &str, value: &str, box_h: f32) {
        self.text(label, x, self.y - 9.0, true, 8.0, LABEL);
        self.y -= 12.0;
        self.ensure(box_h);
        self.box_(x, self.y, w, box_h);
        let lines = wrap(value, false, 9.0, w - 10.0);
        let bottom = self.y - box_h;
        let mut ly = self.y - 12.0;
        for ln in lines {
            if ly < bottom + 4.0 {
                break;
            }
            self.text(&ln, x + 5.0, ly, false, 9.0, INK);
            ly -= 12.0;
        }
        self.y -= box_h;
    }
}

/// Build the full report PDF and return its bytes.
pub fn build_pdf(data: &ReportData, logo: Option<&Logo>) -> Vec<u8> {
    let mut r = Renderer::new(logo.is_some());
    if logo.is_some() {
        r.draw_logo();
    }

    // ===== Header =====
    let header_h = 56.0;
    let cx = PAGE_W / 2.0;
    let center = |r: &mut Renderer, s: &str, y: f32, bold: bool, size: f32, c: (f32, f32, f32)| {
        let x = cx - width_of(s, bold, size) / 2.0;
        r.text(s, x, y, bold, size, c);
    };
    let hy = r.y;
    center(&mut r, "City of Edmonton", hy - 12.0, false, 10.0, INK);
    center(&mut r, "DAILY CONSTRUCTION PROGRESS REPORT", hy - 28.0, true, 15.0, INK);
    center(&mut r, "IIS DEPARTMENT", hy - 41.0, false, 8.5, SUBTLE);
    // logo placeholder rectangle is drawn later as image XObject; reserve space
    r.y -= header_h + 4.0;

    // ===== Top fields =====
    let gap = 8.0;
    let w3 = (CONTENT_W - gap * 2.0) / 3.0;
    r.field(LEFT, w3, "Date:", data.t("date"));
    r.field(LEFT + w3 + gap, w3, "Contract No.", data.t("contractNo"));
    r.field(LEFT + 2.0 * (w3 + gap), w3, "Network Activity No.", data.t("networkActivityNo"));
    r.y -= 24.0;

    r.field(LEFT, CONTENT_W, "Location (Stage):", data.t("location"));
    r.y -= 24.0;

    let w_time = 120.0;
    let w_main = CONTENT_W - 2.0 * (w_time + gap);
    r.field(LEFT, w_main, "Inspector:", data.t("inspector"));
    r.field(LEFT + w_main + gap, w_time, "Start", data.t("inspStart"));
    r.field(LEFT + w_main + gap + w_time + gap, w_time, "End", data.t("inspEnd"));
    r.y -= 24.0;

    r.field_multi(LEFT, CONTENT_W, "Overtime Justification:", data.t("overtime"), 26.0);
    r.y -= 6.0;

    r.field(LEFT, w_main, "Contractor:", data.t("contractor"));
    r.field(LEFT + w_main + gap, w_time, "Start", data.t("contrStart"));
    r.field(LEFT + w_main + gap + w_time + gap, w_time, "End", data.t("contrEnd"));
    r.y -= 24.0;

    r.field(LEFT, w_main, "Type of Weather", data.t("weather"));
    r.field(LEFT + w_main + gap, w_time, "High", data.t("tempHigh"));
    r.field(LEFT + w_main + gap + w_time + gap, w_time, "Low", data.t("tempLow"));
    r.y -= 26.0;

    // ===== Contractors workforce =====
    r.band("CONTRACTORS WORKFORCE", None);
    let sub_w = CONTENT_W / (WF_COLS as f32 * 2.0);
    let name_h = 15.0;
    let subh_h = 13.0;
    r.ensure(name_h + subh_h);
    for c in 0..WF_COLS {
        let nx = LEFT + c as f32 * 2.0 * sub_w;
        r.fill_rect(nx, r.y, sub_w * 2.0, name_h, GRAYHDR);
        r.stroke_rect(nx, r.y, sub_w * 2.0, name_h, BORDER, 0.75);
        let nm_owned = data.t(&format!("wf_name_{c}")).trim().to_string();
        let nm = if nm_owned.is_empty() { WF_DEFAULT_NAMES[c] } else { &nm_owned };
        let nm = fit(nm, true, 8.0, sub_w * 2.0 - 4.0);
        r.vtext(&nm, nx, r.y, sub_w * 2.0, name_h, true, 8.0, LABEL, true);
    }
    r.y -= name_h;
    for c in 0..WF_COLS * 2 {
        let sx = LEFT + c as f32 * sub_w;
        r.fill_rect(sx, r.y, sub_w, subh_h, GRAYHDR);
        r.stroke_rect(sx, r.y, sub_w, subh_h, BORDER, 0.75);
        let lab = if c % 2 == 0 { "Crew" } else { "Equipment" };
        r.vtext(lab, sx, r.y, sub_w, subh_h, true, 7.0, LABEL, true);
    }
    r.y -= subh_h;
    for row in 0..WF_ROWS {
        // pre-wrap all 10 cells, compute row height
        let mut cells: Vec<Vec<String>> = Vec::with_capacity(WF_COLS * 2);
        let mut max_lines = 1usize;
        for col in 0..WF_COLS {
            let crew = data.t(&format!("wf_{col}_{row}_crew"));
            let equip = data.t(&format!("wf_{col}_{row}_equip"));
            let lc = wrap(crew, false, 7.0, sub_w - 5.0);
            let le = wrap(equip, false, 7.0, sub_w - 5.0);
            max_lines = max_lines.max(lc.len().max(1)).max(le.len().max(1));
            cells.push(lc);
            cells.push(le);
        }
        let row_h = (13.0_f32).max(max_lines as f32 * 8.4 + 4.0);
        r.ensure(row_h);
        for (i, lines) in cells.iter().enumerate() {
            let cxp = LEFT + i as f32 * sub_w;
            r.stroke_rect(cxp, r.y, sub_w, row_h, BORDER, 0.75);
            let mut ly = r.y - 9.0;
            for ln in lines {
                r.text(ln, cxp + 3.0, ly, false, 7.0, INK);
                ly -= 8.4;
            }
        }
        r.y -= row_h;
    }
    r.y -= 10.0;

    // ===== Work in progress =====
    r.band("WORK IN PROGRESS", Some("Lab Notified"));
    r.text(WIP_LEGEND, LEFT, r.y - 7.0, false, 7.0, SUBTLE);
    r.y -= 13.0;
    let lab_w = 78.0;
    let desc_w = CONTENT_W - lab_w;
    for i in 0..WIP_ROWS {
        let dtxt = data.t(&format!("wip_{i}_text"));
        let dlines = wrap(dtxt, false, 9.0, desc_w - 10.0);
        let wr_h = (18.0_f32).max(dlines.len() as f32 * 11.0 + 7.0);
        r.ensure(wr_h);
        r.stroke_rect(LEFT, r.y, desc_w, wr_h, BORDER, 0.75);
        r.fill_rect(LEFT + desc_w, r.y, lab_w, wr_h, GRAYHDR);
        r.stroke_rect(LEFT + desc_w, r.y, lab_w, wr_h, BORDER, 0.75);
        let mut dy = r.y - 12.0;
        for ln in &dlines {
            r.text(ln, LEFT + 5.0, dy, false, 9.0, INK);
            dy -= 11.0;
        }
        let cb_top = r.y - (wr_h - 11.0) / 2.0 - 0.5;
        r.checkbox(LEFT + desc_w + lab_w / 2.0 - 5.5, cb_top, 11.0, data.c(&format!("wip_{i}_lab")));
        r.y -= wr_h;
    }
    r.y -= 10.0;

    // ===== Work outside contract =====
    r.band("WORK OUTSIDE CONTRACT, NOTEWORTHY CONVERSATIONS, DAMAGE TO PROPERTY, ETC", None);
    let notes = wrap(data.t("workOutside"), false, 9.0, CONTENT_W - 12.0);
    let notes_h = (70.0_f32).max(notes.len() as f32 * 12.0 + 14.0);
    r.ensure(notes_h);
    r.box_(LEFT, r.y, CONTENT_W, notes_h);
    let mut ny = r.y - 13.0;
    for ln in &notes {
        r.text(ln, LEFT + 6.0, ny, false, 9.0, INK);
        ny -= 12.0;
    }
    r.y -= notes_h + 10.0;

    // ===== Checklist (kept together on a page) =====
    let est = estimate_checklist() + 24.0;
    if r.space() < est {
        r.new_page();
    }
    r.band("CHECKLIST OF ACTIVITIES ON SITE", None);
    let col_gap = 16.0;
    let col_w = (CONTENT_W - col_gap) / 2.0;
    let start_y = r.y;
    let left_end = draw_checklist_col(&mut r, LEFT, col_w, start_y, CK_LEFT, data);
    let right_end = draw_checklist_col(&mut r, LEFT + col_w + col_gap, col_w, start_y, CK_RIGHT, data);
    r.y = left_end.min(right_end) - 10.0;

    // ===== Signatures =====
    r.y -= 6.0;
    r.ensure((20.0 + 12.0) * 2.0);
    draw_sig(&mut r, "Inspector Signature:", data.t("inspectorSig"), data.t("inspectorSigDate"));
    r.y -= 12.0;
    draw_sig(&mut r, "Project Manager Signature:", data.t("pmSig"), data.t("pmSigDate"));

    // flush last page
    r.new_page();

    // ===== assemble the PDF =====
    assemble(r.pages, logo, r.has_logo)
}

fn estimate_checklist() -> f32 {
    let col_h = |groups: &[crate::model::CkGroup]| -> f32 {
        let mut h = 0.0;
        for g in groups {
            h += 16.0 + g.items.len() as f32 * 13.0 + 2.0;
        }
        h
    };
    col_h(CK_LEFT).max(col_h(CK_RIGHT))
}

fn draw_checklist_col(r: &mut Renderer, x: f32, w: f32, top: f32, groups: &[crate::model::CkGroup], data: &ReportData) -> f32 {
    let mut y = top;
    for g in groups {
        r.text(g.head, x, y - 9.0, true, 8.5, NAVY);
        r.line(x, y - 12.0, x + w, y - 12.0, BORDER, 0.6);
        y -= 16.0;
        for (name, label) in g.items {
            r.checkbox(x + 2.0, y, 10.0, data.c(name));
            r.text(label, x + 16.0, y - 8.0, false, 8.5, INK);
            y -= 13.0;
        }
        y -= 2.0;
    }
    y
}

fn draw_sig(r: &mut Renderer, label: &str, sig: &str, date: &str) {
    let h = 20.0;
    let date_box_w = 100.0;
    let lw = width_of(label, true, 9.0) + 6.0;
    r.text(label, LEFT, r.y - h / 2.0 - 9.0 * 0.35, true, 9.0, LABEL);
    let date_label_w = width_of("Date:", true, 9.0) + 4.0;
    let sig_box_x = LEFT + lw;
    let sig_box_w = CONTENT_W - lw - 8.0 - date_label_w - date_box_w;
    r.box_(sig_box_x, r.y, sig_box_w, h);
    let sv = fit(sig, false, 10.0, sig_box_w - 8.0);
    r.vtext(&sv, sig_box_x + 2.0, r.y, sig_box_w, h, false, 10.0, INK, false);
    let dlx = sig_box_x + sig_box_w + 8.0;
    r.text("Date:", dlx, r.y - h / 2.0 - 9.0 * 0.35, true, 9.0, LABEL);
    let dbx = dlx + date_label_w;
    r.box_(dbx, r.y, date_box_w, h);
    let dv = fit(date, false, 10.0, date_box_w - 8.0);
    r.vtext(&dv, dbx + 2.0, r.y, date_box_w, h, false, 10.0, INK, false);
    r.y -= h;
}

/// Write all pages + fonts + (optional) logo image into a finished PDF.
fn assemble(pages: Vec<Content>, logo: Option<&Logo>, has_logo: bool) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog = Ref::new(1);
    let tree = Ref::new(2);
    let font_reg = Ref::new(3);
    let font_bold = Ref::new(4);
    let logo_ref = Ref::new(5);

    // first dynamic id after the fixed ones
    let mut next: i32 = 6;
    let mut alloc = || {
        let r = Ref::new(next);
        next += 1;
        r
    };

    // page + content refs
    let page_refs: Vec<Ref> = pages.iter().map(|_| alloc()).collect();
    let content_refs: Vec<Ref> = pages.iter().map(|_| alloc()).collect();

    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree).kids(page_refs.iter().copied()).count(page_refs.len() as i32);

    // fonts (base-14, WinAnsi)
    pdf.type1_font(font_reg)
        .base_font(Name(b"Helvetica"))
        .encoding_predefined(Name(b"WinAnsiEncoding"));
    pdf.type1_font(font_bold)
        .base_font(Name(b"Helvetica-Bold"))
        .encoding_predefined(Name(b"WinAnsiEncoding"));

    // logo image (DCTDecode = embed JPEG bytes directly, only on page 1)
    if let Some(l) = logo {
        let mut img = pdf.image_xobject(logo_ref, l.jpeg);
        img.filter(Filter::DctDecode);
        img.width(l.w);
        img.height(l.h);
        img.color_space().device_rgb();
        img.bits_per_component(8);
    }

    // write each page
    for (i, content) in pages.into_iter().enumerate() {
        {
            let mut page = pdf.page(page_refs[i]);
            page.parent(tree);
            page.media_box(Rect::new(0.0, 0.0, PAGE_W, PAGE_H));
            page.contents(content_refs[i]);
            let mut res = page.resources();
            {
                let mut fonts = res.fonts();
                fonts.pair(F_REG, font_reg);
                fonts.pair(F_BOLD, font_bold);
            }
            if has_logo && i == 0 {
                res.x_objects().pair(Name(b"Logo"), logo_ref);
            }
        }

        pdf.stream(content_refs[i], &content.finish());
    }

    pdf.finish().to_vec()
}

/// Comment marker under which the editable form data is stored inside the PDF.
pub const MARKER: &str = "%%DCPR-DATA:";

/// Append the form data to a finished PDF as a trailing comment (base64 JSON).
/// PDF viewers ignore bytes after `%%EOF`, so this is invisible but recoverable.
pub fn append_data_marker(pdf: &mut Vec<u8>, data: &ReportData) {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let b64 = STANDARD.encode(to_json(data).as_bytes());
    pdf.extend_from_slice(b"\n");
    pdf.extend_from_slice(MARKER.as_bytes());
    pdf.extend_from_slice(b64.as_bytes());
    pdf.extend_from_slice(b"\n");
}

/// Recover the form data previously stored by `append_data_marker`.
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

// re-export for the json (de)serializer used by both wasm + tests
pub fn to_json(data: &ReportData) -> String {
    let mut map = serde_json::Map::new();
    for (k, v) in &data.text {
        map.insert(k.clone(), serde_json::Value::String(v.clone()));
    }
    for k in &data.checks {
        map.insert(k.clone(), serde_json::Value::Bool(true));
    }
    serde_json::Value::Object(map).to_string()
}

pub fn from_json(s: &str) -> ReportData {
    let mut data = ReportData::default();
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(s) {
        for (k, v) in map {
            match v {
                serde_json::Value::String(s) => {
                    data.text.insert(k, s);
                }
                serde_json::Value::Bool(true) => {
                    data.checks.insert(k);
                }
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
        assert!(bytes.len() > 3000, "pdf too small");
        assert!(bytes.starts_with(b"%PDF"), "missing PDF header");
        std::fs::write("tests/out.pdf", &bytes).unwrap();
        // json round-trip
        let again = to_json(&data);
        let reparsed = from_json(&again);
        assert_eq!(reparsed.text.get("inspector"), data.text.get("inspector"));
        assert_eq!(reparsed.c("conc_forms"), data.c("conc_forms"));
    }

    #[test]
    fn marker_round_trips_through_pdf_bytes() {
        let json = std::fs::read_to_string("tests/sample_data.json").expect("sample json");
        let data = from_json(&json);
        let mut bytes = build_pdf(&data, None);
        append_data_marker(&mut bytes, &data);
        let recovered = extract_data_marker(&bytes).expect("marker recovered");
        assert_eq!(recovered.text.get("location"), data.text.get("location"));
        assert_eq!(recovered.text.get("contractor"), data.text.get("contractor"));
        assert_eq!(recovered.c("barr_arrival"), true);
        assert_eq!(recovered.checks.len(), data.checks.len());
    }
}
