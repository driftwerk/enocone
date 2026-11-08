(() => {
  "use strict";
  const DATA = JSON.parse(document.getElementById("enocone-data").textContent);
  const run = DATA.run;
  const executed = run.executed !== false;
  const flows = DATA.flows;
  const byId = new Map(flows.map((f) => [f.flow.id, f]));
  const app = document.getElementById("app");

  // ---------------------------------------------------------------- dom helpers
  function h(tag, attrs, ...kids) {
    const el = document.createElement(tag);
    if (attrs) {
      for (const [k, v] of Object.entries(attrs)) {
        if (v == null || v === false) continue;
        if (k === "class") el.className = v;
        else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
        else if (k === "style") el.setAttribute("style", v);
        else el.setAttribute(k, v === true ? "" : String(v));
      }
    }
    for (const kid of kids.flat(Infinity)) {
      if (kid == null || kid === false) continue;
      el.append(kid instanceof Node ? kid : document.createTextNode(String(kid)));
    }
    return el;
  }
  const svg = (path, size = 16, cls = "") => {
    const ns = "http://www.w3.org/2000/svg";
    const s = document.createElementNS(ns, "svg");
    s.setAttribute("width", size); s.setAttribute("height", size); s.setAttribute("viewBox", "0 0 24 24");
    s.setAttribute("fill", "none"); s.setAttribute("stroke", "currentColor"); s.setAttribute("stroke-width", "2.4");
    s.setAttribute("stroke-linecap", "round"); s.setAttribute("aria-hidden", "true");
    if (cls) s.setAttribute("class", cls);
    for (const d of [].concat(path)) { const p = document.createElementNS(ns, "path"); p.setAttribute("d", d); s.append(p); }
    return s;
  };
  const ICON = { chev: "m9 6 6 6-6 6", check: "m5 12 5 5 9-10", x: ["m7 7 10 10", "M17 7 7 17"], dash: "M7 12h10", search: ["M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14z", "m20 20-3.5-3.5"], back: "m15 6-6 6 6 6" };

  // Tiny, safe Markdown: paragraphs, lists, **bold**, `code`, [links](url).
  function inline(text) {
    const out = [];
    const re = /(\*\*([^*]+)\*\*|`+([^`]+)`+|\[([^\]]+)\]\(([^)\s]+)\))/g;
    let last = 0, m;
    while ((m = re.exec(text))) {
      if (m.index > last) out.push(text.slice(last, m.index));
      if (m[2]) out.push(h("strong", null, m[2]));
      else if (m[3]) out.push(h("code", null, m[3]));
      else if (/^(https?:|mailto:|#|\/)/.test(m[5])) out.push(h("a", { href: m[5], target: m[5].startsWith("#") ? null : "_blank", rel: "noopener" }, m[4]));
      else out.push(m[4]);
      last = re.lastIndex;
    }
    if (last < text.length) out.push(text.slice(last));
    return out;
  }
  function md(text) {
    const box = h("div", { class: "md" });
    const item = /^\s*([-*]|\d+\.)\s+/;
    const fence = /^(\s*)(`{3,}|~{3,})/;
    const row = /^\s*\|.*\|\s*$/;
    const rule = /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/;
    const cells = (l) => l.trim().replace(/^\||\|$/g, "").split("|").map((c) => c.trim());
    const lines = String(text || "").split("\n");
    let i = 0;
    while (i < lines.length) {
      const l = lines[i];
      if (!l.trim()) { i++; continue; }
      const f = l.match(fence);
      if (f) {
        // A fenced block, kept as written (it may itself contain fences, blank lines or tables).
        const [, indent, mark] = f;
        const body = [];
        i++;
        while (i < lines.length && !(lines[i].trim().startsWith(mark[0].repeat(mark.length)) && lines[i].trim().replace(/[`~]/g, "") === "")) {
          body.push(lines[i].startsWith(indent) ? lines[i].slice(indent.length) : lines[i]);
          i++;
        }
        i++;
        box.append(h("pre", null, h("code", null, body.join("\n"))));
        continue;
      }
      if (row.test(l) && i + 1 < lines.length && rule.test(lines[i + 1])) {
        // A Markdown table: header, alignment rule, rows.
        const align = cells(lines[i + 1]).map((c) => (c.endsWith(":") ? "num" : null));
        const head = cells(l);
        const body = [];
        i += 2;
        while (i < lines.length && row.test(lines[i])) body.push(cells(lines[i++]));
        box.append(h("table", null,
          h("thead", null, h("tr", null, head.map((c, j) => h("th", { class: align[j] }, inline(c))))),
          h("tbody", null, body.map((r) => h("tr", null, head.map((_, j) => h("td", { class: align[j] }, inline(r[j] || ""))))))));
        continue;
      }
      const block = [];
      while (i < lines.length && lines[i].trim() && !fence.test(lines[i]) && !(row.test(lines[i]) && rule.test(lines[i + 1] || ""))) block.push(lines[i++]);
      if (item.test(block[0])) {
        // List items may wrap onto following lines.
        const items = [];
        for (const b of block) { if (item.test(b)) items.push(b.replace(item, "")); else items[items.length - 1] += " " + b.trim(); }
        box.append(h(/^\s*\d+\./.test(block[0]) ? "ol" : "ul", null, items.map((t) => h("li", null, inline(t)))));
      } else {
        box.append(h("p", null, inline(block.map((b) => b.trim()).join(" "))));
      }
    }
    return box;
  }

  const fmtMs = (ms) => (ms == null ? "" : ms < 1000 ? `${ms}ms` : ms < 60000 ? `${(ms / 1000).toFixed(1)}s` : `${Math.floor(ms / 60000)}m ${Math.round((ms % 60000) / 1000)}s`);
  const statusLabel = (s) => ({ passed: "Passed", failed: "Failed", flaky: "Flaky", skipped: "Skipped", pending: "Pending", "not-run": executed ? "Not run" : "Not run" }[s] || s);
  const stageLabel = { draft: "Draft", review: "In review", approved: "Approved", implemented: "Implemented" };
  const lastAttempt = (fr) => fr.attempts[fr.attempts.length - 1] || { steps: [] };
  const allSteps = (flow) => flow.sections.flatMap((s) => s.steps.map((st) => ({ ...st, section: s.kind })));
  function stepResults(fr, attemptIdx) {
    const a = fr.attempts[attemptIdx == null ? fr.attempts.length - 1 : attemptIdx];
    const m = new Map();
    if (a) for (const r of a.steps) m.set(r.key, r);
    return m;
  }
  function failedStep(fr) {
    const res = stepResults(fr);
    return allSteps(fr.flow).find((s) => res.get(s.key)?.status === "failed");
  }
  const stage = (fr) => fr.flow.meta.stage || "implemented";

  // ---------------------------------------------------------------- search
  const hay = new Map(flows.map((fr) => {
    const f = fr.flow, a = lastAttempt(fr);
    const parts = [f.title, f.id, f.path, f.why, f.meta.area, f.meta.owner, f.meta.ticket, (f.meta.tags || []).join(" "), stage(fr), fr.status, ...(f.questions || [])];
    for (const s of allSteps(f)) parts.push(s.intent, s.notes);
    for (const r of a.steps || []) { if (r.error) parts.push(r.error.message); if (r.note) parts.push(r.note); }
    for (const n of f.notes || []) parts.push(n.heading, n.markdown);
    return [f.id, parts.filter(Boolean).join(" \n").toLowerCase()];
  }));

  // ---------------------------------------------------------------- state
  const state = { q: "", status: "all", area: "all", stage: "all", open: new Set() };
  try { const saved = JSON.parse(sessionStorage.getItem("enocone-ui") || "{}"); Object.assign(state, { q: saved.q || "", status: saved.status || "all", area: saved.area || "all", stage: saved.stage || "all" }); } catch (_) {}
  const save = () => { try { sessionStorage.setItem("enocone-ui", JSON.stringify({ q: state.q, status: state.status, area: state.area, stage: state.stage })); } catch (_) {} };

  function route() {
    const p = new URLSearchParams(location.hash.slice(1));
    const id = p.get("flow");
    if (id && byId.has(id)) renderFlow(byId.get(id), p.get("step"), p.get("attempt"));
    else renderIndex();
  }
  window.addEventListener("hashchange", () => { route(); window.scrollTo(0, 0); });

  // ---------------------------------------------------------------- header
  function header(extra) {
    const meta = [h("b", null, run.project)];
    if (executed) {
      meta.push(h("span", null, `Run ${run.id}`));
      if (run.prototype) meta.push(h("span", { class: "proto-tag", title: "Browser steps ran against the screens and storyboards, not the real app" }, "Prototype run"));
      if (run.git_branch || run.git_commit) meta.push(h("span", { class: "mono" }, `${run.git_branch || ""}${run.git_commit ? " @ " + run.git_commit.slice(0, 7) : ""}`));
      meta.push(h("span", null, `env: ${run.env}`), h("span", null, run.driver));
      if (run.started_at) meta.push(h("span", null, new Date(run.started_at).toLocaleString()));
      meta.push(h("span", null, fmtMs(run.duration_ms)));
    } else meta.push(h("span", null, `Flow documentation · ${flows.length} flows`));
    const toggle = h("button", { type: "button", "aria-label": "Switch light or dark theme", onclick: () => {
      const r = document.documentElement; const dark = r.dataset.theme ? r.dataset.theme === "dark" : matchMedia("(prefers-color-scheme: dark)").matches;
      r.dataset.theme = dark ? "light" : "dark"; try { localStorage.setItem("enocone-theme", r.dataset.theme); } catch (_) {}
    } }, "Theme");
    return h("header", { class: "top" }, h("div", { class: "in" },
      h("a", { class: "brand", href: HUB ? HUB.home : "#", "aria-label": HUB ? HUB.site : "enocone" }, logo(), wordmark()),
      HUB ? h("nav", { class: "crumbs", "aria-label": "Breadcrumb" }, h("a", { href: HUB.project_url }, "← " + HUB.project), h("a", { href: HUB.review_url }, "Review")) : null,
      h("nav", { class: "meta", "aria-label": "Run" }, meta),
      h("div", { class: "top-actions" }, extra, run.ci_url ? h("a", { href: run.ci_url, target: "_blank", rel: "noopener" }, "CI run") : null, toggle)));
  }
  // The mark: two cones meeting at a ring, the "c" in the middle of e·n·o·c·o·n·e.
  function logo() {
    const ns = "http://www.w3.org/2000/svg", s = document.createElementNS(ns, "svg");
    s.setAttribute("width", "28"); s.setAttribute("height", "28"); s.setAttribute("viewBox", "0 0 48 48"); s.setAttribute("aria-hidden", "true");
    const el = (tag, attrs) => { const e = document.createElementNS(ns, tag); for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v); return e; };
    s.append(
      el("path", { d: "M5 10 L21 24 L5 38 Z", fill: "#8F7BFF" }), el("ellipse", { cx: 5, cy: 24, rx: 3, ry: 14, fill: "#B9ACFF" }),
      el("path", { d: "M43 10 L27 24 L43 38 Z", fill: "#8F7BFF" }), el("ellipse", { cx: 43, cy: 24, rx: 3, ry: 14, fill: "#B9ACFF" }),
      el("circle", { cx: 24, cy: 24, r: 4.2, fill: "none", stroke: "#FFFFFF", "stroke-width": 2 }));
    return s;
  }
  function wordmark() {
    return h("span", { class: "display wordmark" }, "eno", h("em", null, "c"), "one");
  }

  // ---------------------------------------------------------------- index
  function matches(fr) {
    if (state.status !== "all" && fr.status !== state.status) return false;
    if (state.area !== "all" && (fr.flow.meta.area || "General") !== state.area) return false;
    if (state.stage !== "all" && stage(fr) !== state.stage) return false;
    if (state.q) {
      const t = hay.get(fr.flow.id);
      return state.q.toLowerCase().split(/\s+/).filter(Boolean).every((w) => t.includes(w));
    }
    return true;
  }

  function renderIndex() {
    document.title = `${run.project} · ${executed ? "enocone report" : "flow documentation"}`;
    const s = DATA.summary;
    const counts = { all: flows.length, failed: s.failed, flaky: s.flaky, pending: s.pending || 0, passed: s.passed, skipped: s.skipped };
    const stages = {};
    for (const fr of flows) stages[stage(fr)] = (stages[stage(fr)] || 0) + 1;
    const areas = [...new Set(flows.map((f) => f.flow.meta.area || "General"))].sort();

    let headline, subline;
    if (!executed) {
      headline = "Business flows"; subline = `${flows.length} flows · ${Object.entries(stages).map(([k, v]) => `${v} ${stageLabel[k].toLowerCase()}`).join(" · ")}`;
    } else if (s.failed) {
      headline = `${s.failed} business flow${s.failed > 1 ? "s" : ""} failed`; subline = `${s.passed + s.flaky} of ${s.total} behaved as documented${s.flaky ? ` · ${s.flaky} needed a retry` : ""}${s.pending ? ` · ${s.pending} pending implementation` : ""}`;
    } else if (s.pending) {
      headline = `${s.passed + s.flaky} flows passing, ${s.pending} pending`; subline = "Pending flows describe agreed behaviour that is not fully implemented yet.";
    } else {
      headline = `All ${s.total} business flows passed`; subline = `${s.steps} steps checked${s.flaky ? ` · ${s.flaky} needed a retry` : ""}`;
    }

    const pct = (n) => (s.total ? (100 * n) / s.total : 0);
    const summary = h("section", { "aria-labelledby": "sum", style: "display:flex;flex-direction:column;gap:14px" },
      h("div", { style: "display:flex;flex-wrap:wrap;align-items:baseline;gap:6px 16px" }, h("div", { style: "flex-basis:100%" , class: "kicker" }, executed ? "Run report" : "Flow documents"), h("h1", { id: "sum", class: "big display" }, headline), h("p", { class: "sub" }, subline)),
      executed ? h("div", { class: "bar", role: "img", "aria-label": `${s.passed} passed, ${s.flaky} flaky, ${s.failed} failed, ${s.pending || 0} pending, ${s.skipped} skipped` },
        h("div", { style: `width:${pct(s.passed)}%;background:var(--pass)` }), h("div", { style: `width:${pct(s.flaky)}%;background:var(--flaky)` }),
        h("div", { style: `width:${pct(s.failed)}%;background:var(--fail)` }), h("div", { style: `width:${pct(s.pending || 0)}%;background:var(--pend)` }),
        h("div", { style: `width:${pct(s.skipped)}%;background:var(--skip)` })) : null,
      h("div", { class: "tiles" }, (executed
        ? [["all", "Flows", counts.all], ["passed", "Passed", counts.passed], ["failed", "Failed", counts.failed], ["flaky", "Flaky", counts.flaky], ["pending", "Pending", counts.pending], ["skipped", "Skipped", counts.skipped]]
        : Object.keys(stageLabel).filter((k) => stages[k]).map((k) => ["stage:" + k, stageLabel[k], stages[k]])
      ).map(([k, label, n]) => {
        const isStage = k.startsWith("stage:");
        const pressed = isStage ? state.stage === k.slice(6) : state.status === k;
        return h("button", { type: "button", class: "tile", "aria-pressed": pressed ? "true" : "false", onclick: () => {
          if (isStage) state.stage = pressed ? "all" : k.slice(6); else state.status = pressed ? "all" : k;
          save(); renderIndex();
        } }, h("div", { class: "k", style: k === "failed" && n ? "color:var(--fail-fg)" : k === "pending" && n ? "color:var(--pend-fg)" : "" }, label), h("div", { class: "v" }, n));
      })));

    const fails = flows.filter((f) => f.status === "failed");
    const attention = fails.length ? h("section", { class: "card attn", "aria-labelledby": "attn" },
      h("div", { class: "hd" }, h("h2", { id: "attn" }, "Needs attention"), h("span", { class: "small", style: "color:inherit" }, `${fails.length} failure${fails.length > 1 ? "s" : ""}, with the step that broke`)),
      fails.map((fr) => {
        const st = failedStep(fr); const r = st && stepResults(fr).get(st.key);
        return h("a", { class: "row", href: `#flow=${encodeURIComponent(fr.flow.id)}${st ? "&step=" + st.key : ""}` },
          h("div", { style: "flex:1 1 380px;min-width:0;display:flex;flex-direction:column;gap:3px" }, h("strong", null, fr.flow.title),
            h("span", { class: "small" }, st ? `Step ${st.number ?? ""} · ${st.intent}` : "Cleanup failed")),
          h("div", { class: "err", style: "flex:1 1 340px;min-width:0;align-self:center" }, (r && r.error && r.error.message) || "Failed"));
      })) : null;

    const input = h("input", { type: "search", value: state.q, placeholder: "Search flows, steps, errors, tags, owners, questions…", "aria-label": "Search flows",
      oninput: (e) => { state.q = e.target.value; save(); drawGroups(); } });
    const statusChips = executed ? [["all", "All"], ["failed", "Failed"], ["flaky", "Flaky"], ["pending", "Pending"], ["passed", "Passed"], ["skipped", "Skipped"]]
      .filter(([k]) => k === "all" || counts[k]).map(([k, label]) => h("button", { type: "button", class: "chip", "aria-pressed": state.status === k ? "true" : "false",
        onclick: () => { state.status = k; save(); renderIndex(); } }, label, h("span", { class: "n" }, counts[k]))) : [];
    const areaSel = h("select", { class: "chip", "aria-label": "Filter by area", onchange: (e) => { state.area = e.target.value; save(); drawGroups(); } },
      h("option", { value: "all" }, "Area: all"), areas.map((a) => h("option", { value: a, selected: state.area === a }, a)));
    const stageSel = h("select", { class: "chip", "aria-label": "Filter by stage", onchange: (e) => { state.stage = e.target.value; save(); renderIndex(); } },
      h("option", { value: "all" }, "Stage: all"), Object.keys(stageLabel).filter((k) => stages[k]).map((k) => h("option", { value: k, selected: state.stage === k }, stageLabel[k])));

    const groupsBox = h("div", { style: "display:flex;flex-direction:column;gap:14px" });
    function drawGroups() {
      groupsBox.replaceChildren();
      const shown = flows.filter(matches);
      if (!shown.length) { groupsBox.append(h("div", { class: "empty" }, state.q ? `No flows match “${state.q}”. Search covers titles, step sentences, errors, tags, owners and open questions.` : "No flows match these filters.")); return; }
      for (const area of areas) {
        const list = shown.filter((f) => (f.flow.meta.area || "General") === area);
        if (!list.length) continue;
        const all = flows.filter((f) => (f.flow.meta.area || "General") === area);
        const bits = [`${all.length} flow${all.length > 1 ? "s" : ""}`];
        for (const k of ["failed", "flaky", "pending"]) { const n = all.filter((f) => f.status === k).length; if (n) bits.push(`${n} ${k}`); }
        groupsBox.append(h("div", { class: "card group" }, h("div", { class: "gh" }, h("h3", null, area), h("span", { class: "small" }, bits.join(" · "))), list.map(flowRow)));
      }
    }
    function flowRow(fr) {
      const f = fr.flow, open = state.open.has(f.id), st = stage(fr);
      const box = h("div", { class: "frow" });
      const btn = h("button", { type: "button", "aria-expanded": open ? "true" : "false", onclick: () => { open ? state.open.delete(f.id) : state.open.add(f.id); box.replaceWith(flowRow(fr)); } },
        h("span", { class: `badge s-${fr.status}` }, statusLabel(fr.status)),
        h("span", { class: "ftitle" }, f.title),
        st !== "implemented" ? h("span", { class: "stage" }, stageLabel[st]) : null,
        h("span", { class: "tags" }, (f.meta.tags || []).map((t) => h("span", { class: "tag" }, t))),
        h("span", { class: "dur" }, executed ? (fr.skip_reason ? "" : fmtMs(fr.duration_ms)) : `${allSteps(f).length} steps`),
        svg(ICON.chev, 16, "chev"));
      box.append(btn);
      if (open) {
        const res = stepResults(fr);
        box.append(h("div", { class: "peek" }, h("div", { class: "box" },
          f.why ? h("div", { class: "why", style: "font-size:14px" }, md(f.why)) : null,
          fr.skip_reason ? h("div", { class: "note pend" }, fr.skip_reason) : null,
          f.sections.map((sec) => h("div", null, h("div", { class: "lbl", style: "margin:6px 0 2px" }, sec.kind), sec.steps.map((s) => {
            const r = res.get(s.key) || { status: executed ? "skipped" : "not-run" };
            return [h("div", { class: "mini", style: "padding:5px 0" }, h("span", { class: `dot d-${s.blocks.length ? r.status : "pending"}`, title: statusLabel(r.status) }),
              h("span", { class: "num" }, s.number ? `${s.number}.` : "–"), h("span", { class: "txt" }, s.intent),
              h("span", { class: "small mono" }, s.blocks.map((b) => b.kind).join(" + ") || "not implemented"), h("span", { class: "small", style: "width:56px;text-align:right" }, r.duration_ms ? fmtMs(r.duration_ms) : "")),
            r.error ? h("div", { class: "err", style: "margin:0 0 6px 46px" }, r.error.message) : null];
          }))),
          (f.questions || []).length ? h("div", { class: "note pend" }, `${f.questions.length} open question${f.questions.length > 1 ? "s" : ""}`) : null,
          h("div", { style: "display:flex;flex-wrap:wrap;gap:8px 20px;font-size:13px" }, h("a", { href: `#flow=${encodeURIComponent(f.id)}` }, "Open the full flow document →"), h("span", { class: "small mono" }, f.path)))));
      }
      return box;
    }
    drawGroups();

    app.replaceChildren(header(null), h("main", { class: "wrap" }, summary, attention,
      h("section", { "aria-labelledby": "fl", style: "display:flex;flex-direction:column;gap:12px" },
        h("div", { class: "toolbar" }, h("h2", { id: "fl", class: "display", style: "margin:0 10px 0 0;font-size:22px" }, "Flows"),
          h("label", { class: "search" }, svg(ICON.search, 18), input, h("kbd", null, "/"))),
        h("div", { class: "chips", role: "group", "aria-label": "Filters" }, statusChips, h("span", { style: "flex:1" }), areaSel, stageSel),
        groupsBox),
      footer()));
  }

  function footer() {
    return h("footer", { class: "foot" }, h("span", null, `Generated by enocone ${run.enocone_version} · ${DATA.schema}`), h("span", null, HUB ? `Published on ${HUB.site} · runs are kept ${HUB.retention_days} days` : "A self-contained file: opens offline, straight from CI artifacts."));
  }


  // ---------------------------------------------------------------- designs
  const SVGNS = "http://www.w3.org/2000/svg";
  function sv(tag, attrs, ...kids) {
    const el = document.createElementNS(SVGNS, tag);
    for (const [k, v] of Object.entries(attrs || {})) if (v != null) el.setAttribute(k, v);
    for (const k of kids.flat()) if (k != null) el.append(k instanceof Node ? k : document.createTextNode(String(k)));
    return el;
  }
  function wrap(text, max) {
    const words = String(text).split(/\s+/), lines = [];
    let cur = "";
    for (const w of words) { if ((cur + " " + w).trim().length > max && cur) { lines.push(cur); cur = w; } else cur = (cur + " " + w).trim(); }
    if (cur) lines.push(cur);
    return lines.slice(0, 4);
  }
  // A small layered layout: rank by longest path, order by barycentre, draw as SVG.
  function renderChart(chart, statusOf, flowId) {
    const nodes = chart.nodes.map((n) => ({ ...n }));
    const byId = new Map(nodes.map((n) => [n.id, n]));
    const edges = chart.edges.filter((e) => byId.has(e.from) && byId.has(e.to));
    const out = new Map(nodes.map((n) => [n.id, []])), inc = new Map(nodes.map((n) => [n.id, []]));
    // Drop back edges (cycles) for ranking.
    const state = new Map(), forward = [];
    const visit = (id) => { state.set(id, 1); for (const e of edges.filter((x) => x.from === id)) { const t = state.get(e.to); if (t === 1) continue; forward.push(e); if (!t) visit(e.to); } state.set(id, 2); };
    for (const n of nodes) if (!state.get(n.id)) visit(n.id);
    for (const e of forward) { out.get(e.from).push(e.to); inc.get(e.to).push(e.from); }
    const rank = new Map();
    const rk = (id, seen = new Set()) => { if (rank.has(id)) return rank.get(id); if (seen.has(id)) return 0; seen.add(id); const r = inc.get(id).length ? Math.max(...inc.get(id).map((p) => rk(p, seen) + 1)) : 0; rank.set(id, r); return r; };
    nodes.forEach((n) => rk(n.id));
    const ranks = [];
    nodes.forEach((n, i) => { n.order = i; const r = rank.get(n.id); (ranks[r] = ranks[r] || []).push(n); });
    for (let pass = 0; pass < 2; pass++) for (let r = 1; r < ranks.length; r++) {
      const pos = new Map(ranks[r - 1].map((n, i) => [n.id, i]));
      ranks[r].forEach((n) => { const ps = inc.get(n.id).map((p) => pos.get(p)).filter((x) => x != null); n.bc = ps.length ? ps.reduce((a, b) => a + b, 0) / ps.length : n.order; });
      ranks[r].sort((a, b) => a.bc - b.bc);
    }
    const vertical = chart.direction === "TD" || chart.direction === "BT";
    for (const n of nodes) {
      n.lines = wrap(n.label, n.shape === "diamond" ? 16 : 22);
      const longest = Math.max(...n.lines.map((l) => l.length));
      n.w = Math.max(n.shape === "diamond" ? 140 : 112, longest * 7.4 + (n.shape === "diamond" ? 56 : 32));
      n.h = n.lines.length * 18 + (n.shape === "diamond" ? 44 : 26);
    }
    for (const e of edges) e.lines = e.label ? wrap(e.label, 18) : [];
    const longestLabel = Math.max(0, ...edges.flatMap((e) => e.lines.map((l) => l.length)));
    const GAP_MAIN = vertical ? 64 : Math.max(64, longestLabel * 6.4 + 52), GAP_CROSS = 22, PAD = 20;
    const size = ranks.map((list) => Math.max(...list.map((n) => (vertical ? n.h : n.w))));
    const span = ranks.map((list) => list.reduce((a, n) => a + (vertical ? n.w : n.h), 0) + GAP_CROSS * (list.length - 1));
    const maxSpan = Math.max(...span);
    let main = PAD;
    ranks.forEach((list, r) => {
      let cross = PAD + (maxSpan - span[r]) / 2;
      for (const n of list) {
        const along = main + (size[r] - (vertical ? n.h : n.w)) / 2;
        if (vertical) { n.x = cross; n.y = along; cross += n.w + GAP_CROSS; } else { n.x = along; n.y = cross; cross += n.h + GAP_CROSS; }
      }
      main += size[r] + GAP_MAIN;
    });
    const W = (vertical ? maxSpan : main - GAP_MAIN) + PAD * 2, H = (vertical ? main - GAP_MAIN : maxSpan) + PAD * 2;
    const mid = `ah${Math.random().toString(36).slice(2, 8)}`;
    const root = sv("svg", { viewBox: `0 0 ${W} ${H}`, width: W, class: "chart-svg", role: "presentation", style: `min-width:${Math.min(W, Math.round(W * 0.75))}px` },
      sv("defs", null, sv("marker", { id: mid, markerWidth: 9, markerHeight: 9, refX: 8, refY: 4.5, orient: "auto" }, sv("path", { d: "M0,0 L9,4.5 L0,9 z", class: "chart-arrow" }))));
    for (const e of edges) {
      const a = byId.get(e.from), b = byId.get(e.to);
      const back = rank.get(e.to) <= rank.get(e.from);
      let d;
      if (vertical) {
        const x1 = a.x + a.w / 2, y1 = a.y + a.h, x2 = b.x + b.w / 2, y2 = b.y - 2;
        d = back ? `M${a.x + a.w} ${a.y + a.h / 2} C ${a.x + a.w + 60} ${a.y + a.h / 2}, ${b.x + b.w + 60} ${b.y + b.h / 2}, ${b.x + b.w + 2} ${b.y + b.h / 2}`
                 : `M${x1} ${y1} C ${x1} ${y1 + GAP_MAIN / 2}, ${x2} ${y2 - GAP_MAIN / 2}, ${x2} ${y2}`;
      } else {
        const x1 = a.x + a.w, y1 = a.y + a.h / 2, x2 = b.x - 2, y2 = b.y + b.h / 2;
        d = back ? `M${a.x + a.w / 2} ${a.y + a.h} C ${a.x + a.w / 2} ${a.y + a.h + 50}, ${b.x + b.w / 2} ${b.y + b.h + 50}, ${b.x + b.w / 2} ${b.y + b.h + 2}`
                 : `M${x1} ${y1} C ${x1 + GAP_MAIN / 2} ${y1}, ${x2 - GAP_MAIN / 2} ${y2}, ${x2} ${y2}`;
      }
      const path = sv("path", { d, class: `chart-edge${e.dashed ? " dashed" : ""}${e.thick ? " thick" : ""}`, "marker-end": `url(#${mid})` });
      root.append(path);
      if (e.label) {
        // Midpoint of the cubic: (P0 + 3·P1 + 3·P2 + P3) / 8.
        const p = d.match(/-?[\d.]+/g).map(Number);
        const mx = (p[0] + 3 * p[2] + 3 * p[4] + p[6]) / 8, my = (p[1] + 3 * p[3] + 3 * p[5] + p[7]) / 8;
        const tw = Math.max(...e.lines.map((l) => l.length)) * 6.4 + 12, th = e.lines.length * 15 + 6;
        root.append(sv("g", { class: "chart-elabel" }, sv("rect", { x: mx - tw / 2, y: my - th / 2, width: tw, height: th, rx: 6 }),
          e.lines.map((l, i) => sv("text", { x: mx, y: my - th / 2 + 15 + i * 15, "text-anchor": "middle" }, l))));
      }
    }
    for (const n of nodes) {
      const st = n.step ? statusOf(n.step) : "picture";
      const label = n.step ? `Step ${n.number}: ${st} — ${n.label}` : n.label;
      const g = sv("g", { class: `chart-node s-${st}${n.step ? " linked" : ""}`, role: "img", "aria-label": label, tabindex: n.step ? 0 : null });
      g.append(sv("title", null, label));
      const { x, y, w, h: hh } = n;
      if (n.shape === "diamond") g.append(sv("path", { d: `M${x + w / 2} ${y} L${x + w} ${y + hh / 2} L${x + w / 2} ${y + hh} L${x} ${y + hh / 2} Z`, class: "chart-shape" }));
      else if (n.shape === "circle") g.append(sv("ellipse", { cx: x + w / 2, cy: y + hh / 2, rx: w / 2, ry: hh / 2, class: "chart-shape" }));
      else g.append(sv("rect", { x, y, width: w, height: hh, rx: n.shape === "stadium" ? hh / 2 : n.shape === "round" ? 16 : 10, class: "chart-shape" }));
      n.lines.forEach((l, i) => g.append(sv("text", { x: x + w / 2, y: y + hh / 2 + (i - (n.lines.length - 1) / 2) * 18 + 5, "text-anchor": "middle", class: i === 0 ? "t1" : "t2" }, l)));
      if (n.step) {
        const badge = st === "passed" ? "✓" : st === "failed" ? "✗" : st === "skipped" || st === "not-run" ? "–" : String(n.number);
        g.append(sv("circle", { cx: x + w - 2, cy: y + 2, r: 11, class: "chart-badge" }), sv("text", { x: x + w - 2, y: y + 6.5, "text-anchor": "middle", class: "chart-badge-t" }, badge));
        const go = () => { location.hash = `flow=${encodeURIComponent(flowId)}&step=${encodeURIComponent(n.step)}`; };
        g.addEventListener("click", go);
        g.addEventListener("keydown", (ev) => { if (ev.key === "Enter") go(); });
      }
      root.append(g);
    }
    const legend = executed
      ? h("div", { class: "chart-legend small" }, [["passed", "Passed"], ["failed", "Failed"], ["pending", "Pending"], ["skipped", "Not run"]].map(([k, l]) => h("span", null, h("i", { class: `sw s-${k}` }), l)), h("span", null, "Click a box to open its step"))
      : h("div", { class: "chart-legend small" }, h("span", null, h("i", { class: "sw s-planned" }), "Planned: boxes take their step's result after a run"));
    // A chart wider than its box scrolls sideways: say so, and shade the edge with more to see.
    const scroller = h("div", { class: "chart-scroll" }, root);
    const hint = h("div", { class: "chart-hint small", hidden: true }, "Scroll sideways to see the whole chart →");
    const check = () => {
      const more = scroller.scrollWidth > scroller.clientWidth + 2;
      hint.hidden = !more;
      scroller.classList.toggle("more-right", more && scroller.scrollLeft + scroller.clientWidth < scroller.scrollWidth - 2);
      scroller.classList.toggle("more-left", more && scroller.scrollLeft > 2);
    };
    scroller.addEventListener("scroll", check, { passive: true });
    if (window.ResizeObserver) new ResizeObserver(check).observe(scroller);
    requestAnimationFrame(check);
    return h("figure", { class: "chart", role: "figure", "aria-label": "Flow chart" }, scroller, hint, legend);
  }

  function screenDoc(html) {
    const theme = DATA.theme || "";
    const base = `<meta charset="utf-8"><style>html,body{margin:0}body{font-family:Inter,system-ui,sans-serif}</style><style>${theme}</style>`;
    if (/<head[\s>]/i.test(html)) return html.replace(/<head([^>]*)>/i, `<head$1>${base}`);
    return `<!doctype html><html><head>${base}</head><body>${html}</body></html>`;
  }
  function screenFrame(sc, cls) {
    const name = sc.title || sc.name || "mockup";
    return h("iframe", { class: cls || "mock", sandbox: "", srcdoc: screenDoc(sc.html || ""), title: `Mockup: ${name}`, loading: "lazy", tabindex: -1 });
  }
  // A frame drawn at the browser's viewport (as screenshots are) and scaled to fit.
  function viewportFrame(sc) {
    const VW = 1280, VH = 720;
    const frame = screenFrame(sc, "mock vp");
    frame.style.width = VW + "px"; frame.style.height = VH + "px";
    const box = h("div", { class: "vpbox" }, frame);
    const fit = () => { const w = box.clientWidth; if (w) frame.style.transform = `scale(${w / VW})`; };
    if (window.ResizeObserver) new ResizeObserver(fit).observe(box);
    requestAnimationFrame(fit);
    return box;
  }
  const winbar = (url) => h("div", { class: "winbar", "aria-hidden": "true" }, h("i"), h("i"), h("i"), h("span", { class: "url" }, url || ""));
  const screenUrl = (sc) => (sc.path ? sc.path.replace(/\{(\w+)\}/g, (_, k) => k) : `screen: ${sc.name || "mockup"}`);
  function screenFigure(sc, caption) {
    const name = sc.title || sc.name || "mockup";
    return h("figure", { class: "mockfig", role: "figure", "aria-label": `Mockup: ${sc.name || name}` },
      h("div", { class: "mockwrap" }, winbar(screenUrl(sc)), viewportFrame(sc)),
      h("figcaption", { class: "small" }, caption || "Design", " · ", name));
  }
  function stepDesigns(s, r) {
    const screens = (s.designs || []).filter((d) => d.screen).map((d) => d.screen);
    const charts = (s.designs || []).filter((d) => d.chart);
    if (!screens.length && !charts.length) return null;
    const parts = [];
    if (screens.length && r && r.screenshot) {
      parts.push(h("div", { class: "dvb", role: "group", "aria-label": "Designed and built" },
        h("figure", { class: "mockfig", role: "figure", "aria-label": `Designed: ${screens[0].name || "mockup"}` }, h("div", { class: "mockwrap" }, winbar(screenUrl(screens[0])), viewportFrame(screens[0])), h("figcaption", { class: "small" }, h("strong", null, "Designed"), " · the mockup agreed in review")),
        h("figure", { class: "mockfig built", role: "figure", "aria-label": "Built: screenshot from this run" }, h("div", { class: "mockwrap" }, winbar(r.page_url || ""), h("img", { src: r.screenshot, alt: `Built: ${s.intent}`, onclick: (e) => zoom(e.target.src) })), h("figcaption", { class: "small" }, h("strong", null, "Built"), " · screenshot from this run"))));
      screens.slice(1).forEach((sc) => parts.push(screenFigure(sc)));
    } else screens.forEach((sc) => parts.push(screenFigure(sc)));
    return h("div", { class: "designs" }, parts);
  }
  function renderStoryboard(d, screensByName) {
    const ts = d.transitions || [];
    const tos = new Set(ts.map((t) => t.to));
    const start = (ts.find((t) => !tos.has(t.from)) || ts[0] || {}).from;
    const chain = [], used = new Set();
    let cur = start;
    while (cur && !chain.includes(cur)) {
      chain.push(cur);
      const t = ts.find((x) => x.from === cur && !used.has(x));
      if (!t) break;
      used.add(t); t._chain = true; cur = t.to;
    }
    const thumb = (name) => {
      const sc = screensByName.get(name) || { name, html: `<p style="padding:24px;color:#999">No screen “${name}”</p>` };
      return h("figure", { class: "sb-thumb", role: "figure", "aria-label": `Screen: ${name}` }, h("div", { class: "sb-frame" }, screenFrame(sc, "mock sb-mock")), h("figcaption", { class: "small" }, sc.title || name));
    };
    const arrow = (t) => {
      const what = t.on ? Object.entries(t.on).map(([k, v]) => `${k} “${typeof v === "string" ? v : JSON.stringify(v)}”`).join(", ") : "";
      return h("div", { class: "sb-arrow", role: "img", "aria-label": `Step ${t.step ?? "?"}: ${t.from} to ${t.to}${what ? " — " + what : ""}` },
        h("span", { class: "sb-n" }, t.step ?? "→"), h("span", { class: "sb-line" }), h("span", { class: "small" }, what));
    };
    const row = [];
    chain.forEach((name, i) => { if (i) row.push(arrow(ts.find((t) => t.from === chain[i - 1] && t.to === name && t._chain))); row.push(thumb(name)); });
    const extra = ts.filter((t) => !t._chain);
    return h("figure", { class: "storyboard", role: "figure", "aria-label": "Storyboard" },
      h("div", { class: "sb-row" }, row),
      extra.length ? h("ul", { class: "sb-extra small" }, extra.map((t) => h("li", { role: "img", "aria-label": `Step ${t.step ?? "?"}: ${t.from} to ${t.to}` }, `${t.from} → ${t.to}`, t.step ? ` (step ${t.step})` : ""))) : null);
  }
  function flowDesigns(f, statusOf) {
    const screensByName = new Map();
    for (const st of allSteps(f)) for (const d of st.designs || []) if (d.screen && d.screen.name) screensByName.set(d.screen.name, d.screen);
    for (const d of f.designs || []) for (const sc of [d.screen, ...(d.screens || [])]) if (sc && sc.name && !screensByName.has(sc.name)) screensByName.set(sc.name, sc);
    const inBoards = new Set((f.designs || []).flatMap((d) => (d.transitions || []).flatMap((t) => [t.from, t.to])));
    const out = [];
    for (const d of f.designs || []) {
      if (d.chart) out.push(renderChart(d.chart, statusOf, f.id));
      else if ((d.transitions || []).length) out.push(renderStoryboard(d, screensByName));
      else if (d.screen && d.source && !inBoards.has(d.screen.name)) out.push(screenFigure(d.screen));
    }
    return out.length ? h("section", { class: "flow-designs", "aria-label": "Designs" }, out) : null;
  }

  // ---------------------------------------------------------------- flow document
  // Template noise shortened for one-line summaries.
  const tidy = (t) => t
    .replace(/"?\{\{\s*env\.ENOCONE_BIN\s*\}\}"?/g, "enocone")
    .replace(/\{\{\s*env\.FIXTURES\s*\}\}/g, "fixtures")
    .replace(/\{\{\s*run\.tmp\s*\}\}/g, "tmp")
    .replace(/\{\{\s*env\.(\w+)\s*\}\}/g, "$1")
    .replace(/"/g, "");

  // A block's one-line title: its own `# comment` on the first line, else a summary.
  function blockTitle(b) {
    const lines = (b.source || "").split("\n").map((l) => l.trim());
    const own = lines.find((l) => l);
    if (own && own.startsWith("#")) return own.replace(/^#+\s*/, "");
    // A command means little to a reviewer: without a title, say so (the command is in the details).
    if (b.kind === "exec") return h("span", { class: "untitled" }, "Runs a command (no description)");
    const t = blockSummary(b);
    return t.length > 80 ? t.slice(0, 79) + "…" : t;
  }

  function blockSummary(b) {
    const first = (b.source || "").split("\n").map((l) => l.trim()).filter((l) => l && !l.startsWith("#"));
    if (b.kind === "http") {
      const m = first.find((l) => /^(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\s*:/i.test(l)) || first[0] || "";
      const auth = first.find((l) => /^auth\s*:/.test(l));
      return m.replace(/\s*:\s*/, " ").replace(/^"|"$/g, "").replace(/"/g, "") + (auth ? ` · as ${auth.split(":")[1].trim()}` : "");
    }
    if (b.kind === "use") return `includes ${first[0] || ""}`.replace(/:$/, "");
    if (b.kind === "email") {
      const to = first.find((l) => /^to\s*:/.test(l));
      const subj = first.find((l) => /^subject\s*:/.test(l));
      const clean = (l) => l.replace(/^\w+\s*:\s*/, "").replace(/^"|"$/g, "");
      return `waits for email to ${to ? clean(to) : "?"}${subj ? ` · “${clean(subj)}”` : ""}`;
    }
    if (b.kind === "exec") {
      const i = first.findIndex((l) => /^run\s*:/.test(l));
      let cmd = i < 0 ? first[0] || "" : first[i].replace(/^run\s*:\s*/, "");
      let more = 0;
      if (/^[|>][-+]?$/.test(cmd)) {
        // A script: its first command, or the comment that says what it does.
        const script = [];
        for (const l of (b.source || "").split("\n").slice((b.source || "").split("\n").findIndex((x) => /^\s*run\s*:/.test(x)) + 1)) {
          if (/^\S/.test(l)) break;
          if (l.trim()) script.push(l.trim());
        }
        const note = script.find((l) => l.startsWith("#"));
        if (note) return note.replace(/^#+\s*/, "");
        const cmds = script.filter((l) => !l.startsWith("#"));
        cmd = cmds[0] || "";
        more = cmds.length - 1;
      }
      if (cmd.startsWith("- ")) cmd = first.slice(i + 1).filter((l) => l.startsWith("- ")).map((l) => l.slice(2)).join(" ");
      // Drop leading VAR=value settings so the command itself shows.
      cmd = tidy(cmd).replace(/^(\w+=("[^"]*"|'[^']*'|\S*)\s+)+/, "").replace(/^env(\s+-u\s+\S+)*\s+(\w+=\S+\s+)*/, "")
        .replace(/\s*\d?>+\s*\/dev\/null/g, "").replace(/\s*2>&1/g, "").trim();
      return "$ " + cmd + (more > 0 ? `  (+${more} more)` : "");
    }
    const acts = first.filter((l) => l.startsWith("- ")).map((l) => l.slice(2));
    const as = first.find((l) => /^as\s*:/.test(l));
    if (b.kind === "expect") return first.join(" ").slice(0, 120);
    if (!acts.length) return first.join(" ").slice(0, 100);
    // Where it goes, and how much it does there.
    const go = acts.find((a) => a.startsWith("goto:"));
    let where = go ? tidy(go.replace(/^goto:\s*/, "")) : acts[0].split(":")[0];
    where = where.replace(/^https?:\/\/(127\.0\.0\.1|localhost)(:\d+)?/, "").replace(/^\{\{[^}]+\}\}$/, "a link");
    return (as ? `as ${as.split(":")[1].trim()} · ` : "") + (go ? `goto ${where}` : where) + (acts.length > 1 ? ` · ${acts.length} actions` : "");
  }

  // Which records each block produced: http, exec and email blocks make one, a browser
  // block a run of actions. Null when they can't be matched up (then shown together).
  function recordsPerBlock(blocks, recs) {
    const out = [];
    let i = 0;
    for (const b of blocks) {
      const mine = [];
      if (b.kind === "browser") { while (i < recs.length && recs[i].type === "action") mine.push(recs[i++]); }
      else if (["http", "exec", "email"].includes(b.kind)) { if (i < recs.length && recs[i].type === b.kind) mine.push(recs[i++]); }
      else return null;
      out.push(mine);
    }
    return i === recs.length ? out : null;
  }

  function happened(r) {
    const lines = [];
    const push = (t, c) => lines.push(c ? h("span", { class: c }, t + "\n") : t + "\n");
    for (const rec of r.records || []) {
      if (rec.type === "http") {
        push(`${rec.method} ${rec.url}`, null);
        for (const [k, v] of rec.request_headers || []) push(`${k}: ${v}`, "dim");
        if (rec.request_body) push(rec.request_body, null);
        if (rec.polls && rec.polls.length > 1) {
          push("");
          for (const p of rec.polls) push(`+${fmtMs(p.at_ms).padEnd(6)} ← ${p.status ?? "error"}  ${p.summary}`, "dim");
        }
        push("");
        push(`← ${rec.status ?? "no response"} · ${fmtMs(rec.duration_ms)}`, rec.status && rec.status < 400 ? "ok" : "bad");
        if (rec.response_body) push(rec.response_body, null);
        if (rec.checks && rec.checks.length) { push(""); for (const c of rec.checks) push(`${c.ok ? "✓" : "✗"} ${c.what}${c.detail ? " — " + c.detail : ""}`, c.ok ? "ok" : "bad"); }
        push("");
      } else if (rec.type === "email") {
        push(`✉ to ${rec.to} · ${rec.mailbox}`, null);
        if (rec.subject != null) {
          push(`  subject: ${rec.subject}`, null);
          if (rec.from) push(`  from: ${rec.from}`, "dim");
          if (rec.received_at) push(`  received: ${rec.received_at}`, "dim");
          push("");
          if (rec.text) push(rec.text, null);
          if (rec.links && rec.links.length) { push(""); for (const l of rec.links) push(`  link: ${l}`, "dim"); }
        } else {
          push("  no matching message", "bad");
          for (const m of rec.seen || []) push(`  had: ${m}`, "dim");
        }
        push(`← ${fmtMs(rec.duration_ms)}`, rec.subject != null ? "ok" : "bad");
        if (rec.checks && rec.checks.length) for (const c of rec.checks) push(`${c.ok ? "✓" : "✗"} ${c.what}${c.detail ? " — " + c.detail : ""}`, c.ok ? "ok" : "bad");
        push("");
      } else if (rec.type === "exec") {
        push(`$ ${rec.command}`, null);
        if (rec.cwd) push(`  in ${rec.cwd}`, "dim");
        push("");
        if (rec.stdout) push(rec.stdout.replace(/\n$/, ""), null);
        if (rec.stderr) push(rec.stderr.replace(/\n$/, ""), "dim");
        push(`← exit ${rec.exit ?? "none"} · ${fmtMs(rec.duration_ms)}`, rec.exit === 0 ? "ok" : "bad");
        if (rec.checks && rec.checks.length) for (const c of rec.checks) push(`${c.ok ? "✓" : "✗"} ${c.what}${c.detail ? " — " + c.detail : ""}`, c.ok ? "ok" : "bad");
        push("");
      } else {
        push(`${rec.ok ? "✓" : "✗"} ${rec.description}  ${fmtMs(rec.duration_ms)}`, rec.ok ? "ok" : "bad");
        if (rec.error) push(`  ${rec.error}`, "bad");
      }
    }
    if (r.captured && r.captured.length) { push(""); for (const [k, v] of r.captured) push(`kept ${k} = ${v}`, "dim"); }
    if (r.page_url) push(`page: ${r.page_url}`, "dim");
    return lines;
  }

  const COMMENTS = DATA.comments || [];
  // Set when the page is served by an enocone site (enoc.one), not opened as a file.
  const HUB = DATA.hub || null;
  function commentsFor(flowId, stepKey) {
    const list = COMMENTS.filter((c) => c.flow === flowId && (stepKey ? c.step === stepKey : !c.step));
    if (!list.length) return null;
    return h("div", { class: "comments", role: "list", "aria-label": "Review comments" }, list.map((c) =>
      h("div", { class: `comment${c.resolved ? " resolved" : ""}`, role: "listitem" },
        h("div", { class: "who" }, c.author || "Reviewer", c.at ? h("span", null, new Date(c.at).toLocaleDateString()) : null, c.resolved ? h("span", null, "resolved") : null),
        md(c.text))));
  }
  function renderStep(s, r, opts) {
    const status = r ? r.status : executed ? "skipped" : "not-run";
    const isPending = !s.blocks.length || status === "pending";
    const markCls = isPending ? "m-pending" : `m-${status}`;
    const markIcon = status === "passed" ? svg(ICON.check, 14) : status === "failed" ? svg(ICON.x, 14) : status === "skipped" ? svg(ICON.dash, 14) : (s.number ?? "–");
    const open = opts.open;
    const detail = h("div", { style: open ? "" : "display:none", class: "sdetail" });
    const fill = () => {
      detail.replaceChildren();
      const recs = r ? r.records || [] : [];
      const per = recordsPerBlock(s.blocks, recs);
      const many = s.blocks.length > 1;
      const tail = r ? { records: [], captured: r.captured, page_url: r.page_url } : null;
      if (per) {
        // One row per block: its title across both columns, then what was written beside what it did.
        detail.append(h("div", { class: "details lbls" }, h("div", { class: "lbl" }, "As written"), recs.length ? h("div", { class: "lbl" }, "What happened") : null));
        s.blocks.forEach((b, i) => {
          const last = i === s.blocks.length - 1;
          const out = per[i].length || (last && tail && (tail.captured || []).length) ? happened(last ? { ...tail, records: per[i] } : { records: per[i] }) : null;
          detail.append(h("div", { class: "blk" },
            many ? h("div", { class: "blk-t" }, h("span", { class: "k" }, b.kind), " ", blockTitle(b)) : null,
            h("div", { class: "details" }, h("pre", { class: "code" }, b.source), out ? h("pre", { class: "term" }, out) : h("div", null))));
        });
      } else {
        const cols = h("div", { class: "details" },
          h("div", null, h("div", { class: "lbl" }, "As written"), s.blocks.map((b) => h("div", { class: "blk" },
            many ? h("div", { class: "blk-t" }, h("span", { class: "k" }, b.kind), " ", blockTitle(b)) : null,
            h("pre", { class: "code" }, b.source)))));
        if (recs.length) cols.append(h("div", null, h("div", { class: "lbl" }, "What happened"), h("pre", { class: "term" }, happened(r))));
        detail.append(cols);
      }
      if (r && r.snapshot) detail.append(h("details", null, h("summary", { class: "small" }, "Accessibility snapshot of the page at failure"), h("pre", { class: "code" }, r.snapshot)));
      if (r && (r.console || []).length) detail.append(h("details", null, h("summary", { class: "small" }, `Browser console (${r.console.length})`), h("pre", { class: "code" }, r.console.join("\n"))));
    };
    if (open) fill();
    const how = s.blocks.length ? h("button", { type: "button", class: "how", "aria-expanded": open ? "true" : "false", onclick: (e) => {
      const btn = e.currentTarget, isOpen = btn.getAttribute("aria-expanded") === "true";
      btn.setAttribute("aria-expanded", isOpen ? "false" : "true");
      if (!isOpen && !detail.childNodes.length) fill();
      detail.style.display = isOpen ? "none" : "";
    } }, svg(ICON.chev, 14, "chev"), h("span", { class: "how-list" },
      s.blocks.map((b) => h("span", { class: "how-t" }, h("span", { class: "k" }, b.kind), " ", blockTitle(b))))) : null;
    const children = r && (r.children || []).length ? h("div", { class: "children" }, r.children.map((c) => {
      const cr = c.result;
      return h("div", { class: `step is-${cr.status}` }, h("span", { class: `mark m-${cr.status}` }, cr.status === "passed" ? svg(ICON.check, 11) : cr.status === "failed" ? svg(ICON.x, 11) : "–"),
        h("div", { class: "sbody" }, h("div", { class: "shead" }, h("span", { class: "intent" }, c.intent), h("span", { class: "small" }, fmtMs(cr.duration_ms))),
          cr.error ? h("div", { class: "err" }, cr.error.message) : null));
    })) : null;
    const notes = commentsFor(opts.flowId, s.key);
    return h("div", { class: `step is-${isPending ? "pending" : status}${opts.target ? " target" : ""}${notes ? " has-comments" : ""}`, id: `step-${s.key}` },
      h("span", { class: `mark ${markCls}`, "aria-label": isPending ? "Pending" : statusLabel(status), title: isPending ? "Pending" : statusLabel(status) }, markIcon),
      h("div", { class: "sbody" },
        h("div", { class: "shead" }, h("span", { class: "intent" }, s.number ? h("span", { class: "n" }, `${s.number}.`) : null, s.intent), r && r.duration_ms ? h("span", { class: "small" }, fmtMs(r.duration_ms)) : null),
        s.notes ? h("div", { class: "note" }, s.notes) : null,
        stepTables(s, r),
        stepDesigns(s, r),
        !s.blocks.length && !opts.designing ? h("div", { class: "note pend" }, "Not implemented yet. This step describes agreed behaviour; an http, browser, expect, use, exec or email block will make it executable.") : null,
        how,
        detail,
        r && r.error ? h("div", { class: "err" }, r.error.message) : null,
        r && r.note && status !== "passed" && s.blocks.length ? h("div", { class: "note" }, r.note) : null,
        children,
        r && r.screenshot && !(s.designs || []).some((d) => d.screen) ? h("figure", { class: "shot" }, h("img", { src: r.screenshot, alt: `Screenshot after: ${s.intent}`, loading: "lazy", onclick: (e) => zoom(e.target.src) }),
          h("figcaption", { class: "small" }, "Screenshot after this step", r.page_url ? ` · ${r.page_url}` : "")) : null), notes);
  }

  // A step's tables of examples: the first with each row's result, then any linked tables.
  function stepTables(s, r) {
    const tables = s.tables || [];
    if (!tables.length) return null;
    const rowRes = new Map(((r && r.rows) || []).map((x) => [x.row, x]));
    // Checks describe the expected value as `= "15%"`; compare the value itself.
    const unquote = (v) => (v == null ? "" : String(v).replace(/^=\s*/, "").replace(/^"(.*)"$/, "$1"));
    const one = (t, ti) => {
      const driving = ti === 0;
      const ran = driving && rowRes.size > 0;
      const head = t.columns.map((c, ci) => (t.link === ci ? null : h("th", { class: c.expected ? "exp" : null, title: c.expected ? "Expected result" : null }, c.header)));
      const body = t.rows.map((row, ri) => {
        const rr = driving ? rowRes.get(ri + 1) : null;
        const st = rr ? rr.result.status : null;
        const err = rr && rr.result.error;
        const cells = t.columns.map((c, ci) => {
          if (t.link === ci) return null;
          const v = row.cells[ci] || "";
          // In a failed row, an expected cell that the failed check compared against shows what came back.
          const same = (a, b) => a === b || (a !== "" && b !== "" && !isNaN(a) && !isNaN(b) && Number(a) === Number(b));
          const bad = st === "failed" && c.expected && err && err.expected != null && same(unquote(err.expected), v.trim());
          return h("td", { class: [c.expected ? "exp" : null, bad ? "bad" : null].filter(Boolean).join(" ") || null }, v, bad && err.actual != null ? h("span", { class: "got" }, ` got ${unquote(err.actual)}`) : null);
        });
        const mark = !ran ? null : h("td", { class: "res" },
          st === "passed" ? [svg(ICON.check, 12), " passed"] : st === "failed" ? [svg(ICON.x, 12), " failed"] : statusLabel(st || "not-run"));
        const tr = h("tr", { class: st ? `r-${st}` : null }, cells, mark);
        if (!rr) return tr;
        // Open a row to see what it sent and got back.
        const more = h("tr", { class: "rdetail", style: "display:none" }, h("td", { colspan: String(t.columns.length + 1) },
          err ? h("div", { class: "err" }, err.message) : null,
          (rr.result.records || []).length ? h("pre", { class: "term" }, happened(rr.result)) : null));
        tr.classList.add("clickable");
        tr.title = "Show what this row sent and got back";
        tr.onclick = () => { more.style.display = more.style.display === "none" ? "" : "none"; };
        return [tr, more];
      });
      return h("div", { class: "xtable" },
        t.name ? h("div", { class: "small tname" }, `Table: ${t.name}`) : null,
        h("table", null, h("thead", null, h("tr", null, head, ran ? h("th", null, "Result") : null)), h("tbody", null, body)));
    };
    return h("div", { class: "xtables" }, tables.map(one));
  }

  function zoom(src) {
    const d = h("dialog", { class: "zoom", onclick: () => d.close(), onclose: () => d.remove() }, h("img", { src, alt: "Screenshot, enlarged" }));
    document.body.append(d); d.showModal();
  }

  function renderFlow(fr, targetStep, attemptParam) {
    const f = fr.flow;
    document.title = `${f.title} · ${run.project}`;
    const attemptIdx = attemptParam != null ? Math.min(Number(attemptParam) - 1, fr.attempts.length - 1) : fr.attempts.length - 1;
    const res = stepResults(fr, attemptIdx);
    const st = stage(fr);
    const steps = allSteps(f);
    const failed = steps.find((s) => res.get(s.key)?.status === "failed");
    let banner = null;
    if (executed) {
      const passedN = steps.filter((s) => res.get(s.key)?.status === "passed").length;
      const pendN = steps.filter((s) => !s.blocks.length || res.get(s.key)?.status === "pending").length;
      if (fr.status === "failed" && failed) banner = h("div", { class: "banner failed", role: "note" }, svg(ICON.x, 18), h("strong", null, `Failed at step ${failed.number ?? ""}: ${failed.intent}`), h("span", { class: "small", style: "color:inherit" }, `${passedN} of ${steps.length} steps passed · ${fmtMs(fr.duration_ms)}`));
      else if (fr.status === "failed") banner = h("div", { class: "banner failed", role: "note" }, h("strong", null, "Cleanup failed"));
      else if (fr.status === "pending") banner = h("div", { class: "banner pending", role: "note" }, h("strong", null, fr.skip_reason ? `Not run: ${fr.skip_reason}` : `${pendN} step${pendN > 1 ? "s" : ""} not implemented yet`), passedN ? h("span", { class: "small", style: "color:inherit" }, `${passedN} implemented step${passedN > 1 ? "s" : ""} passed`) : null);
      else if (fr.status === "skipped") banner = h("div", { class: "banner skipped", role: "note" }, h("strong", null, `Skipped: ${fr.skip_reason || ""}`));
      else if (fr.status === "flaky") banner = h("div", { class: "banner flaky", role: "note" }, h("strong", null, `Passed on attempt ${fr.attempts.length} after failing first`));
      else if (run.prototype) banner = h("div", { class: "banner passed", role: "note" }, svg(ICON.check, 18), h("strong", null, `Works against the prototype · ${steps.length} steps · ${fmtMs(fr.duration_ms)}`), h("span", { class: "small" }, "http and exec steps were skipped: the prototype has no backend"));
      else banner = h("div", { class: "banner passed", role: "note" }, svg(ICON.check, 18), h("strong", null, `Behaved as documented · ${steps.length} steps · ${fmtMs(fr.duration_ms)}`));
    } else if (st !== "implemented") {
      banner = h("div", { class: "banner pending", role: "note" }, h("strong", null, st === "draft" ? "Draft: behaviour still being designed" : st === "review" ? "In review: please read and comment" : "Approved: being implemented"),
        f.meta.approved_by ? h("span", { class: "small", style: "color:inherit" }, `approved by ${f.meta.approved_by}`) : null);
    }
    const meta = f.meta;
    const art = h("article", { class: "doc" },
      h("div", { style: "display:flex;flex-direction:column;gap:12px" },
        h("div", { class: "tags" }, meta.area ? h("span", { class: "tag" }, meta.area) : null, (meta.tags || []).map((t) => h("span", { class: "tag" }, t)),
          meta.priority ? h("span", { class: "tag", style: "color:var(--fail-fg);background:var(--fail-bg);font-weight:600" }, meta.priority) : null,
          h("span", { class: "stage" }, stageLabel[st]),
          meta.owner ? h("span", { class: "small" }, `owner: ${meta.owner}`) : null, meta.ticket ? h("span", { class: "small" }, `ticket: ${meta.ticket}`) : null),
        h("div", { class: "kicker" }, [meta.area || "Flow", stageLabel[st]].join(" · ")),
        h("h1", { class: "display" }, f.title),
        f.why ? h("div", { class: "why" }, md(f.why)) : null,
        banner, commentsFor(f.id, null)),
      flowDesigns(f, (key) => {
        if (!executed) return "planned";
        const step = steps.find((x) => x.key === key);
        const r = res.get(key);
        if (step && !step.blocks.length) return "pending";
        return r ? r.status : "not-run";
      }),
      (f.questions || []).length ? h("section", { class: "questions", "aria-labelledby": "oq" }, h("h2", { id: "oq" }, `Open questions (${f.questions.length})`), h("ol", null, f.questions.map((q) => h("li", null, inline(q))))) : null,
      (f.notes || []).map((n) => h("section", { class: "sec" }, h("h2", null, n.heading), md(n.markdown))),
      f.sections.map((sec) => h("section", { class: "sec", "aria-label": sec.kind }, h("h2", null, sec.kind),
        sec.steps.map((s) => renderStep(s, executed ? res.get(s.key) : null, { flowId: f.id, designing: st === "draft" || st === "review", open: s.key === targetStep || (!targetStep && res.get(s.key)?.status === "failed"), target: s.key === targetStep })))),
      h("details", null, h("summary", { class: "small" }, `Source: ${f.path}`), h("pre", { class: "code", style: "margin-top:8px" }, f.source)));

    const a = fr.attempts[attemptIdx];
    const side = h("aside", { class: "side", "aria-label": "Run details" },
      executed && a && (a.captured || []).length ? h("div", { class: "card" }, h("h2", null, "Values captured in this run"), a.captured.map(([k, v]) => h("div", { class: "kv" }, h("span", null, k), h("span", null, v)))) : null,
      executed && fr.attempts.length > 1 ? h("div", { class: "card" }, h("h2", null, "Attempts"), fr.attempts.map((at, i) => h("a", { href: `#flow=${encodeURIComponent(f.id)}&attempt=${i + 1}`, style: i === attemptIdx ? "font-weight:700" : "" }, `Attempt ${i + 1}: ${statusLabel(at.status)} · ${fmtMs(at.duration_ms)}`))) : null,
      h("div", { class: "card" }, h("h2", null, "About this flow"),
        h("div", { class: "kv" }, h("span", null, "id"), h("span", null, f.id)), h("div", { class: "kv" }, h("span", null, "file"), h("span", null, f.path)),
        h("div", { class: "kv" }, h("span", null, "stage"), h("span", null, stageLabel[st])), h("div", { class: "kv" }, h("span", null, "source sha256"), h("span", null, f.source_hash.slice(0, 12)))),
      h("div", { class: "cmd mono" }, h("div", { style: "font-family:Inter,sans-serif;font-weight:700;margin-bottom:6px;color:#fff" }, executed ? "Run it locally" : "Run it"), `enocone run ${f.path}${executed ? ` --env ${run.env}` : ""}${run.prototype ? " --prototype" : ""} --headed`));

    app.replaceChildren(header(null), h("main", { class: "wrap" }, h("a", { class: "back", href: "#" }, "← All flows"), h("div", { class: "docgrid" }, art, side), footer()));
    if (targetStep) { const el = document.getElementById(`step-${targetStep}`); if (el) setTimeout(() => el.scrollIntoView({ block: "center" }), 0); }
  }

  document.addEventListener("keydown", (e) => {
    if (e.key === "/" && !/^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName || "")) {
      const s = document.querySelector('input[type="search"]'); if (s) { e.preventDefault(); s.focus(); }
    }
  });
  try { const t = localStorage.getItem("enocone-theme"); if (t) document.documentElement.dataset.theme = t; } catch (_) {}
  route();
})();
