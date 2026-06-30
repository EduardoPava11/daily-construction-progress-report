//! Single source of truth for the report's structure: field names, the
//! workforce grid, work-in-progress rows, and the on-site checklist.
//! Used by both the PDF renderer (pure) and the DOM layer (wasm).

/// Number of contractor columns in the workforce grid (Prime + 4 Sub).
pub const WF_COLS: usize = 5;
/// Rows in the workforce grid.
pub const WF_ROWS: usize = 8;
/// Work-in-progress description rows.
pub const WIP_ROWS: usize = 8;

pub const WF_DEFAULT_NAMES: [&str; WF_COLS] = [
    "Prime Contractor",
    "Sub Contractor",
    "Sub Contractor",
    "Sub Contractor",
    "Sub Contractor",
];

pub const WIP_LEGEND: &str =
    "1) Excavation   2) Drainage   3) Subbase/Subgrade   4) Concrete   5) Asphalt   6) Utilities   7) Other";

/// A checklist group: a heading and its (field-name, label) items.
pub struct CkGroup {
    pub head: &'static str,
    pub items: &'static [(&'static str, &'static str)],
}

pub const CK_LEFT: &[CkGroup] = &[
    CkGroup { head: "Barricading:", items: &[
        ("barr_arrival", "Check Site Arrival"),
        ("barr_departure", "Check Site Departure"),
        ("barr_secure", "Contractor Advised to Secure"),
    ]},
    CkGroup { head: "Excavation:", items: &[("exc_truckbox", "Truck Box Measured")] },
    CkGroup { head: "Fill:", items: &[("fill_lift", "Lift Thickness Checked")] },
    CkGroup { head: "Subgrade:", items: &[("sub_elev", "Elevation Checked")] },
    CkGroup { head: "Con. Base:", items: &[
        ("cb_grade", "Check Grade"),
        ("cb_elev", "Elevation Checked (10mm)"),
        ("cb_thick", "Thickness Tolerances Check"),
    ]},
    CkGroup { head: "Concrete:", items: &[
        ("conc_straight", "Straight Edge Tolerance (6mm)"),
        ("conc_elev", "Elevation Tolerance (15mm)"),
        ("conc_cross", "Crossfall Tolerance (10mm)"),
        ("conc_forms", "Check Forms"),
    ]},
    CkGroup { head: "Soil Cement:", items: &[
        ("sc_grade", "Check Grade"),
        ("sc_lift", "Lift Thickness Check"),
    ]},
];

pub const CK_RIGHT: &[CkGroup] = &[
    CkGroup { head: "Drainage:", items: &[
        ("dr_pipe", "Pipe Stamped"),
        ("dr_grade", "Check Grade"),
        ("dr_elev", "Elevation Checked"),
    ]},
    CkGroup { head: "Landscaping:", items: &[("ls_topsoil", "Topsoil Depth")] },
    CkGroup { head: "Pavers:", items: &[
        ("pv_grade", "Check Grade"),
        ("pv_straight", "Straight Edge Tolerance (8mm)"),
        ("pv_joints", "Joints Between Paves (3mm)"),
    ]},
    CkGroup { head: "Asphalt:", items: &[
        ("as_notify", "Notify Lab"),
        ("as_lift", "Lift Checked"),
        ("as_spread", "Spreading Temp"),
    ]},
    CkGroup { head: "FDR:", items: &[
        ("fdr_grade", "Check Grade"),
        ("fdr_cement", "Cement Spread"),
        ("fdr_visual", "Visual Inspection"),
    ]},
];

/// Every checkbox field name (work-in-progress lab flags + checklist items).
pub fn all_checkbox_names() -> Vec<String> {
    let mut v = Vec::new();
    for i in 0..WIP_ROWS {
        v.push(format!("wip_{i}_lab"));
    }
    for g in CK_LEFT.iter().chain(CK_RIGHT.iter()) {
        for (name, _) in g.items {
            v.push((*name).to_string());
        }
    }
    v
}
