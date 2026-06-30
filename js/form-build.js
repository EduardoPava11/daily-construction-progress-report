/* form-build.js
 * Builds the repetitive parts of the form (workforce grid, work-in-progress
 * rows, and the on-site checklist) from data so the markup stays small and
 * every input gets a stable `name` for the generic save/load serializer.
 */
(function () {
  "use strict";

  // ---- Workforce: 5 contractor columns x 8 rows, each cell = Crew + Equipment
  var WF_COLS = 5;
  var WF_ROWS = 8;
  var wfBody = document.getElementById("wfBody");
  for (var r = 0; r < WF_ROWS; r++) {
    var tr = document.createElement("tr");
    for (var c = 0; c < WF_COLS; c++) {
      tr.appendChild(cell("wf_" + c + "_" + r + "_crew"));
      tr.appendChild(cell("wf_" + c + "_" + r + "_equip"));
    }
    wfBody.appendChild(tr);
  }
  function cell(name) {
    var td = document.createElement("td");
    var inp = document.createElement("input");
    inp.type = "text";
    inp.name = name;
    td.appendChild(inp);
    return td;
  }

  // ---- Work in progress: 8 description rows, each with a "Lab Notified" check
  var WIP_ROWS = 8;
  var wipBody = document.getElementById("wipBody");
  for (var i = 0; i < WIP_ROWS; i++) {
    var row = document.createElement("tr");

    var tdText = document.createElement("td");
    tdText.className = "wip-text";
    var txt = document.createElement("input");
    txt.type = "text";
    txt.name = "wip_" + i + "_text";
    tdText.appendChild(txt);

    var tdLab = document.createElement("td");
    tdLab.className = "wip-lab";
    var chk = document.createElement("input");
    chk.type = "checkbox";
    chk.name = "wip_" + i + "_lab";
    tdLab.appendChild(chk);

    row.appendChild(tdText);
    row.appendChild(tdLab);
    wipBody.appendChild(row);
  }

  // ---- Checklist (mirrors the spreadsheet, grouped by activity) -----------
  var LEFT = [
    { group: "Barricading:", items: [
      ["barr_arrival", "Check Site Arrival"],
      ["barr_departure", "Check Site Departure"],
      ["barr_secure", "Contractor Advised to Secure"],
    ]},
    { group: "Excavation:", items: [
      ["exc_truckbox", "Truck Box Measured"],
    ]},
    { group: "Fill:", items: [
      ["fill_lift", "Lift Thickness Checked"],
    ]},
    { group: "Subgrade:", items: [
      ["sub_elev", "Elevation Checked"],
    ]},
    { group: "Con. Base:", items: [
      ["cb_grade", "Check Grade"],
      ["cb_elev", "Elevation Checked (10mm)"],
      ["cb_thick", "Thickness Tolerances Check"],
    ]},
    { group: "Concrete:", items: [
      ["conc_straight", "Straight Edge Tolerance (6mm)"],
      ["conc_elev", "Elevation Tolerance (15mm)"],
      ["conc_cross", "Crossfall Tolerance (10mm)"],
      ["conc_forms", "Check Forms"],
    ]},
    { group: "Soil Cement:", items: [
      ["sc_grade", "Check Grade"],
      ["sc_lift", "Lift Thickness Check"],
    ]},
  ];
  var RIGHT = [
    { group: "Drainage:", items: [
      ["dr_pipe", "Pipe Stamped"],
      ["dr_grade", "Check Grade"],
      ["dr_elev", "Elevation Checked"],
    ]},
    { group: "Landscaping:", items: [
      ["ls_topsoil", "Topsoil Depth"],
    ]},
    { group: "Pavers:", items: [
      ["pv_grade", "Check Grade"],
      ["pv_straight", "Straight Edge Tolerance (8mm)"],
      ["pv_joints", "Joints Between Paves (3mm)"],
    ]},
    { group: "Asphalt:", items: [
      ["as_notify", "Notify Lab"],
      ["as_lift", "Lift Checked"],
      ["as_spread", "Spreading Temp"],
    ]},
    { group: "FDR:", items: [
      ["fdr_grade", "Check Grade"],
      ["fdr_cement", "Cement Spread"],
      ["fdr_visual", "Visual Inspection"],
    ]},
  ];

  buildChecklist(document.getElementById("ckLeft"), LEFT);
  buildChecklist(document.getElementById("ckRight"), RIGHT);

  function buildChecklist(host, groups) {
    groups.forEach(function (g) {
      var gEl = document.createElement("div");
      gEl.className = "ck-group";
      var h = document.createElement("div");
      h.className = "ck-head";
      h.textContent = g.group;
      gEl.appendChild(h);
      g.items.forEach(function (it) {
        var lab = document.createElement("label");
        lab.className = "ck-item";
        var box = document.createElement("input");
        box.type = "checkbox";
        box.name = it[0];
        var span = document.createElement("span");
        span.textContent = it[1];
        lab.appendChild(box);
        lab.appendChild(span);
        gEl.appendChild(lab);
      });
      host.appendChild(gEl);
    });
  }
})();
