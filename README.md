# Daily Construction Progress Report

A static, offline web form that reproduces the City of Edmonton IIS *Daily
Construction Progress Report*. Fill it out, export a faithful vector PDF, and
re-open that PDF later to keep editing. Built in **Rust**, compiled to
**WebAssembly**, hosted on GitHub Pages.

## Live site

**https://eduardopava11.github.io/daily-construction-progress-report/**

Works on desktop and mobile. Your work auto-saves in the browser as you type.

### Buttons

| Button | What it does |
|---|---|
| **Export PDF** | Renders the filled form to a crisp, vector PDF (real text, never an image). Your data is also embedded inside the PDF. |
| **Open PDF...** | Re-opens a PDF this tool exported and re-fills the form so you can fix or add things. |
| **Save Draft** | Saves your data as a small `.json` file. |
| **Load Draft...** | Loads a `.json` draft back in. |
| **Clear** | Empties the whole form. |

## How it works

- **The PDF is drawn as true vector graphics** with the `pdf-writer` crate, so
  text is sharp at any zoom, nothing is clipped, and the output is identical on
  every device. (Earlier image-based exports clipped text; this replaces them.)
- **Re-editing a PDF**: on export, the complete form data is appended to the PDF
  as a hidden `%%DCPR-DATA:` comment (base64 JSON). PDF viewers ignore it, but
  **Open PDF...** reads it back. So the PDF you download is itself the editable
  document. Only PDFs exported by this tool carry that data.
- **No hand-written JavaScript.** All logic is Rust. The only `.js` is the small
  loader that `wasm-bindgen`/Trunk generate to start the WebAssembly module
  (browsers can't run Rust directly; WASM plus a tiny glue loader is the
  platform reality).

## Project layout

```
index.html        static form markup (no JS), Trunk asset links
style.css         styling, mobile layout
src/lib.rs        module wiring; splits pure logic from wasm glue
src/pdf.rs        pure vector PDF renderer  (host-testable via `cargo test`)
src/model.rs      field / checklist / workforce definitions (single source of truth)
src/metrics.rs    Helvetica glyph widths (for text wrapping and centering)
src/app.rs        browser glue (wasm only): DOM <-> data, autosave, export, import
assets/logo.png   logo shown on the web page
assets/logo.jpg   logo embedded in the PDF (DCTDecode)
FIELDS.md         inventory of every input (what you type vs. what you tick)
```

## Build it yourself

Requires the Rust toolchain plus the wasm target and Trunk:

```
rustup target add wasm32-unknown-unknown
cargo install trunk

trunk serve            # local dev at http://127.0.0.1:8080
trunk build --release  # static output in dist/
cargo test             # renders a sample PDF to tests/out.pdf + checks round-trips
```

GitHub Pages is built and deployed automatically by
`.github/workflows/deploy.yml` on every push to `main`.
