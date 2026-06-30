//! Browser glue (wasm only): wires the static HTML form to the pure PDF
//! renderer. Reads/writes field values by `name`, autosaves to localStorage,
//! and handles PDF/JSON export + import.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{
    Document, Element, FileReader, HtmlAnchorElement, HtmlInputElement, HtmlSelectElement,
    HtmlTextAreaElement,
};

use crate::pdf::{self, Logo, ReportData};

const LOGO_JPEG: &[u8] = include_bytes!("../assets/logo.jpg");
const LS_KEY: &str = "dcpr_autosave_v1";

fn document() -> Document {
    web_sys::window().unwrap().document().unwrap()
}

fn status(msg: &str) {
    if let Some(el) = document().get_element_by_id("status") {
        el.set_text_content(Some(msg));
    }
}

// ---- iterate every element carrying a `name` attribute ----
fn for_each_named<F: FnMut(&Element, String)>(mut f: F) {
    let nodes = document().query_selector_all("[name]").unwrap();
    for i in 0..nodes.length() {
        let node = nodes.get(i).unwrap();
        if let Ok(el) = node.dyn_into::<Element>() {
            if let Some(name) = el.get_attribute("name") {
                f(&el, name);
            }
        }
    }
}

// ---- DOM <-> ReportData ----
fn read_form() -> ReportData {
    let mut d = ReportData::default();
    for_each_named(|el, name| {
        if let Some(inp) = el.dyn_ref::<HtmlInputElement>() {
            if inp.type_() == "checkbox" {
                if inp.checked() {
                    d.checks.insert(name);
                }
            } else {
                let v = inp.value();
                if !v.is_empty() {
                    d.text.insert(name, v);
                }
            }
        } else if let Some(ta) = el.dyn_ref::<HtmlTextAreaElement>() {
            let v = ta.value();
            if !v.is_empty() {
                d.text.insert(name, v);
            }
        } else if let Some(se) = el.dyn_ref::<HtmlSelectElement>() {
            let v = se.value();
            if !v.is_empty() {
                d.text.insert(name, v);
            }
        }
    });
    d
}

fn write_form(d: &ReportData) {
    for_each_named(|el, name| {
        if let Some(inp) = el.dyn_ref::<HtmlInputElement>() {
            if inp.type_() == "checkbox" {
                inp.set_checked(d.c(&name));
            } else {
                inp.set_value(d.t(&name));
            }
        } else if let Some(ta) = el.dyn_ref::<HtmlTextAreaElement>() {
            ta.set_value(d.t(&name));
        } else if let Some(se) = el.dyn_ref::<HtmlSelectElement>() {
            se.set_value(d.t(&name));
        }
    });
}

fn clear_form() {
    for_each_named(|el, _name| {
        if let Some(inp) = el.dyn_ref::<HtmlInputElement>() {
            if inp.type_() == "checkbox" {
                inp.set_checked(false);
            } else {
                inp.set_value("");
            }
        } else if let Some(ta) = el.dyn_ref::<HtmlTextAreaElement>() {
            ta.set_value("");
        } else if let Some(se) = el.dyn_ref::<HtmlSelectElement>() {
            se.set_value("");
        }
    });
}

// ---- localStorage autosave ----
fn autosave() {
    let json = pdf::to_json(&read_form());
    if let Ok(Some(store)) = web_sys::window().unwrap().local_storage() {
        let _ = store.set_item(LS_KEY, &json);
    }
    status("Saved");
}

fn load_autosave() {
    if let Ok(Some(store)) = web_sys::window().unwrap().local_storage() {
        if let Ok(Some(json)) = store.get_item(LS_KEY) {
            write_form(&pdf::from_json(&json));
        }
    }
}

// ---- file download ----
fn download(bytes: &[u8], filename: &str, mime: &str) {
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.buffer());
    let mut opts = web_sys::BlobPropertyBag::new();
    opts.type_(mime);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).unwrap();
    let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();
    let doc = document();
    let a: HtmlAnchorElement = doc.create_element("a").unwrap().dyn_into().unwrap();
    a.set_href(&url);
    a.set_download(filename);
    doc.body().unwrap().append_child(&a).unwrap();
    a.click();
    a.remove();
    let _ = web_sys::Url::revoke_object_url(&url);
}

fn report_filename(ext: &str) -> String {
    let d = read_form();
    let mut parts = vec!["DailyReport".to_string()];
    let date = d.t("date");
    if !date.is_empty() {
        parts.push(date.to_string());
    }
    let loc: String = d
        .t("location")
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let loc: String = loc.trim_matches('_').chars().take(40).collect();
    if !loc.is_empty() {
        parts.push(loc);
    }
    format!("{}.{}", parts.join("_"), ext)
}

// ---- export PDF ----
fn export_pdf() {
    let data = read_form();
    autosave();
    let logo = Logo { jpeg: LOGO_JPEG, w: 250, h: 250 };
    let mut bytes = pdf::build_pdf(&data, Some(&logo));
    pdf::append_data_marker(&mut bytes, &data);
    download(&bytes, &report_filename("pdf"), "application/pdf");
    status("Exported PDF");
}

fn save_draft() {
    let json = pdf::to_json(&read_form());
    download(json.as_bytes(), &report_filename("json"), "application/json");
    status("Draft saved");
}

// ---- import (PDF or JSON) via FileReader ----
fn import_file(input: HtmlInputElement, is_pdf: bool) {
    let files = match input.files() {
        Some(f) => f,
        None => return,
    };
    let file = match files.get(0) {
        Some(f) => f,
        None => return,
    };
    let reader = FileReader::new().unwrap();
    let reader_c = reader.clone();
    let input_c = input.clone();
    let onload = Closure::<dyn FnMut()>::new(move || {
        let result = reader_c.result().unwrap();
        if is_pdf {
            let array = js_sys::Uint8Array::new(&result);
            let bytes = array.to_vec();
            match pdf::extract_data_marker(&bytes) {
                Some(d) => {
                    write_form(&d);
                    autosave();
                    status("Loaded from PDF");
                }
                None => {
                    web_sys::window().unwrap().alert_with_message(
                        "This PDF has no editable data. Only PDFs exported by this tool can be re-opened.",
                    ).ok();
                }
            }
        } else {
            let text = result.as_string().unwrap_or_default();
            write_form(&pdf::from_json(&text));
            autosave();
            status("Draft loaded");
        }
        input_c.set_value("");
    });
    reader.set_onload(Some(onload.as_ref().unchecked_ref()));
    onload.forget();
    if is_pdf {
        reader.read_as_array_buffer(&file).unwrap();
    } else {
        reader.read_as_text(&file).unwrap();
    }
}

// ---- listener wiring ----
fn on_click(id: &str, f: impl Fn() + 'static) {
    if let Some(el) = document().get_element_by_id(id) {
        let cb = Closure::<dyn FnMut(_)>::new(move |_e: web_sys::Event| f());
        el.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())
            .unwrap();
        cb.forget();
    }
}

fn on_change_file(id: &str, is_pdf: bool) {
    if let Some(el) = document().get_element_by_id(id) {
        let input: HtmlInputElement = el.dyn_into().unwrap();
        let input_c = input.clone();
        let cb = Closure::<dyn FnMut(_)>::new(move |_e: web_sys::Event| {
            import_file(input_c.clone(), is_pdf);
        });
        input
            .add_event_listener_with_callback("change", cb.as_ref().unchecked_ref())
            .unwrap();
        cb.forget();
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    load_autosave();

    // autosave on any input within the form
    if let Some(form) = document().get_element_by_id("report") {
        let cb = Closure::<dyn FnMut(_)>::new(move |_e: web_sys::Event| autosave());
        form.add_event_listener_with_callback("input", cb.as_ref().unchecked_ref())
            .unwrap();
        cb.forget();
    }

    on_click("exportPdf", export_pdf);
    on_click("saveDraft", save_draft);
    on_click("clearBtn", || {
        if web_sys::window()
            .unwrap()
            .confirm_with_message("Clear the entire form? This cannot be undone.")
            .unwrap_or(false)
        {
            clear_form();
            autosave();
            status("Cleared");
        }
    });
    on_change_file("importPdf", true);
    on_change_file("importJson", false);
}
