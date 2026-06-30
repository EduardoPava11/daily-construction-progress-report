# Daily Construction Progress Report

A static, offline web form that reproduces the City of Edmonton IIS *Daily
Construction Progress Report* spreadsheet. Fill it out, export a PDF, and
re-open that PDF later to keep editing.

## Use it

Double-click **`index.html`** (no server, no internet needed). It works in any
modern browser.

### Buttons (top bar)

| Button | What it does |
|---|---|
| **Export PDF** | Renders the filled form to a PDF and downloads it. The PDF also has your data embedded inside it. |
| **Open PDF…** | Re-opens a PDF you exported here and re-fills the form so you can fix mistakes or add things, then export again. |
| **Save Draft** | Saves your data as a small `.json` file. |
| **Load Draft…** | Loads a `.json` draft back in. |
| **Clear** | Empties the whole form. |

Your work is also **auto-saved in the browser** as you type, so if you close
the tab and come back, it's still there.

## How editing a PDF works

Re-typing data out of a finished PDF is unreliable, so instead each PDF this
tool exports carries a hidden copy of your form data inside it (in the PDF's
*Subject* metadata, plus a visible `report-data.json` attachment). **Open PDF…**
reads that copy back. This means you can only re-edit PDFs that *this tool*
created — a scanned or third-party PDF has no data to read.

## Files

```
index.html          the page
css/style.css        styling / print layout
js/form-build.js     builds the workforce grid, work-in-progress rows, checklist
js/app.js            save / load / autosave / PDF export + import
lib/pdf-lib.min.js   PDF generation + metadata (vendored, offline)
lib/html2canvas.min.js  renders the form to an image (vendored, offline)
assets/logo.png      City of Edmonton logo (from the original template)
```

## Notes

- The PDF page is a high-resolution image of the form, so it prints exactly as
  it looks on screen. (Text in the PDF is therefore not selectable — that's the
  trade-off for pixel-faithful, layout-stable output.)
- You can also use your browser's **File → Print → Save as PDF** for a
  text-selectable version, but that copy won't carry the re-editable data.
