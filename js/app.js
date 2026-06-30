/* app.js
 * Form state (save/load/autosave) + PDF export/import.
 *
 * Round-trip strategy: the exported PDF is rendered as an image (html2canvas)
 * laid out by pdf-lib, and the COMPLETE form state is embedded twice:
 *   1. base64 JSON in the PDF's Subject metadata  -> used to re-open & edit
 *   2. an attached file "report-data.json"        -> visible in Acrobat etc.
 * On "Open PDF", we read the Subject back and repopulate the form, so the
 * PDF you download is itself the editable document.
 */
(function () {
  "use strict";

  var form = document.getElementById("report");
  var statusEl = document.getElementById("status");
  var SUBJECT_PREFIX = "DCPR1::"; // marks our embedded data inside the PDF
  var LS_KEY = "dcpr_autosave_v1";

  // ---------- generic serialize / restore over [name] inputs -------------
  function serialize() {
    var data = {};
    Array.prototype.forEach.call(form.elements, function (el) {
      if (!el.name) return;
      if (el.type === "checkbox") data[el.name] = !!el.checked;
      else data[el.name] = el.value;
    });
    return data;
  }

  function restore(data) {
    if (!data) return;
    Array.prototype.forEach.call(form.elements, function (el) {
      if (!el.name || !(el.name in data)) return;
      if (el.type === "checkbox") el.checked = !!data[el.name];
      else el.value = data[el.name];
    });
  }

  // ---------- autosave to localStorage -----------------------------------
  function autosave() {
    try { localStorage.setItem(LS_KEY, JSON.stringify(serialize())); } catch (e) {}
    flash("Saved");
  }
  var saveTimer = null;
  form.addEventListener("input", function () {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(autosave, 400);
  });

  (function loadAutosave() {
    try {
      var raw = localStorage.getItem(LS_KEY);
      if (raw) restore(JSON.parse(raw));
    } catch (e) {}
  })();

  function flash(msg) {
    statusEl.textContent = msg;
    clearTimeout(flash._t);
    flash._t = setTimeout(function () { statusEl.textContent = ""; }, 1500);
  }

  // ---------- base64 <-> JSON (utf-8 safe) -------------------------------
  function jsonToB64(obj) {
    var s = JSON.stringify(obj);
    var bytes = new TextEncoder().encode(s);
    var bin = "";
    for (var i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
    return btoa(bin);
  }
  function b64ToJson(b64) {
    var bin = atob(b64);
    var bytes = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    return JSON.parse(new TextDecoder().decode(bytes));
  }

  // ---------- file download helper ---------------------------------------
  function download(bytes, filename, mime) {
    var blob = new Blob([bytes], { type: mime });
    var url = URL.createObjectURL(blob);
    var a = document.createElement("a");
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    setTimeout(function () { URL.revokeObjectURL(url); }, 4000);
  }

  function reportFilename(ext) {
    var d = form.elements["date"] && form.elements["date"].value;
    var loc = (form.elements["location"] && form.elements["location"].value || "").trim();
    loc = loc.replace(/[^\w\-]+/g, "_").replace(/^_+|_+$/g, "").slice(0, 40);
    var parts = ["DailyReport"];
    if (d) parts.push(d);
    if (loc) parts.push(loc);
    return parts.join("_") + "." + ext;
  }

  // ---------- Export PDF --------------------------------------------------
  var exportBtn = document.getElementById("exportPdf");
  exportBtn.addEventListener("click", function () {
    runExport().catch(function (err) {
      console.error(err);
      alert("PDF export failed: " + (err && err.message ? err.message : err));
      busy(false);
    });
  });

  function busy(on) {
    exportBtn.disabled = on;
    exportBtn.textContent = on ? "Working…" : "Export PDF";
    document.body.classList.toggle("exporting", on);
  }

  async function runExport() {
    busy(true);
    autosave();
    var sheet = document.getElementById("sheet");

    // High-resolution raster of the form sheet.
    var canvas = await html2canvas(sheet, {
      scale: 2,
      backgroundColor: "#ffffff",
      useCORS: true,
      windowWidth: sheet.scrollWidth,
    });

    var PDFLib = window.PDFLib;
    var pdfDoc = await PDFLib.PDFDocument.create();

    // US Letter portrait, with a margin.
    var PAGE_W = 612, PAGE_H = 792, MARGIN = 24;
    var availW = PAGE_W - MARGIN * 2;
    var availH = PAGE_H - MARGIN * 2;

    // Scale the canvas to page width; slice vertically across pages.
    var scale = availW / canvas.width;
    var scaledFullH = canvas.height * scale;
    var pageSliceH = availH;                       // pts per page (content area)
    var sliceCanvasH = Math.floor(pageSliceH / scale); // canvas px per page
    var numPages = Math.max(1, Math.ceil(scaledFullH / availH));

    for (var p = 0; p < numPages; p++) {
      var srcY = p * sliceCanvasH;
      var srcH = Math.min(sliceCanvasH, canvas.height - srcY);
      if (srcH <= 0) break;

      var slice = document.createElement("canvas");
      slice.width = canvas.width;
      slice.height = srcH;
      slice.getContext("2d").drawImage(canvas, 0, srcY, canvas.width, srcH, 0, 0, canvas.width, srcH);

      var pngBytes = dataUrlToBytes(slice.toDataURL("image/png"));
      var img = await pdfDoc.embedPng(pngBytes);
      var page = pdfDoc.addPage([PAGE_W, PAGE_H]);
      var drawW = availW;
      var drawH = srcH * scale;
      page.drawImage(img, {
        x: MARGIN,
        y: PAGE_H - MARGIN - drawH,
        width: drawW,
        height: drawH,
      });
    }

    // Embed the editable data.
    var data = serialize();
    var b64 = jsonToB64(data);
    pdfDoc.setSubject(SUBJECT_PREFIX + b64);
    pdfDoc.setTitle("Daily Construction Progress Report");
    pdfDoc.setCreator("DCPR Web Form");
    pdfDoc.setProducer("DCPR Web Form");
    try {
      var jsonStr = JSON.stringify(data, null, 2);
      await pdfDoc.attach(new TextEncoder().encode(jsonStr), "report-data.json", {
        mimeType: "application/json",
        description: "Editable form data for the DCPR web form",
      });
    } catch (e) { /* attachment is a bonus; Subject is the source of truth */ }

    var out = await pdfDoc.save();
    download(out, reportFilename("pdf"), "application/pdf");
    busy(false);
    flash("Exported PDF");
  }

  function dataUrlToBytes(dataUrl) {
    var base64 = dataUrl.split(",")[1];
    var bin = atob(base64);
    var bytes = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    return bytes;
  }

  // ---------- Import PDF (re-open & edit) --------------------------------
  document.getElementById("importPdf").addEventListener("change", function (e) {
    var file = e.target.files[0];
    if (!file) return;
    var reader = new FileReader();
    reader.onload = async function () {
      try {
        var pdfDoc = await window.PDFLib.PDFDocument.load(reader.result, { ignoreEncryption: true });
        var subj = pdfDoc.getSubject ? pdfDoc.getSubject() : null;
        if (subj && subj.indexOf(SUBJECT_PREFIX) === 0) {
          var data = b64ToJson(subj.slice(SUBJECT_PREFIX.length));
          restore(data);
          autosave();
          flash("Loaded from PDF");
        } else {
          alert(
            "This PDF doesn't contain editable form data.\n\n" +
            "Only PDFs exported by this tool can be re-opened for editing " +
            "(the data is stored inside them on export)."
          );
        }
      } catch (err) {
        console.error(err);
        alert("Could not read that PDF: " + (err && err.message ? err.message : err));
      } finally {
        e.target.value = "";
      }
    };
    reader.readAsArrayBuffer(file);
  });

  // ---------- Save / Load JSON draft -------------------------------------
  document.getElementById("saveDraft").addEventListener("click", function () {
    var bytes = new TextEncoder().encode(JSON.stringify(serialize(), null, 2));
    download(bytes, reportFilename("json"), "application/json");
    flash("Draft saved");
  });

  document.getElementById("importJson").addEventListener("change", function (e) {
    var file = e.target.files[0];
    if (!file) return;
    var reader = new FileReader();
    reader.onload = function () {
      try {
        restore(JSON.parse(reader.result));
        autosave();
        flash("Draft loaded");
      } catch (err) {
        alert("Could not read that draft file.");
      } finally {
        e.target.value = "";
      }
    };
    reader.readAsText(file);
  });

  // ---------- Clear -------------------------------------------------------
  document.getElementById("clearBtn").addEventListener("click", function () {
    if (!confirm("Clear the entire form? This cannot be undone.")) return;
    Array.prototype.forEach.call(form.elements, function (el) {
      if (!el.name) return;
      if (el.type === "checkbox") el.checked = false;
      else el.value = "";
    });
    autosave();
    flash("Cleared");
  });
})();
