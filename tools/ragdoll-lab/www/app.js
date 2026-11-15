// nv-rs Ragdoll Lab: the page around the WebAssembly lab (src/lib.rs).
//
// URL parameters (for an automated browser; README.md):
//   ?scenario=<name>|all     run a built-in scenario (or all of them) at load
//   ?hits=...&ground=...     run a custom scenario (scenario.rs documents the syntax)
//   &tune=key:value,...      tuning on top of the game's values, for the run and the session
//   &headless=1              no drawing; the results only
// The results land in <pre id="results">, in window.ragdollLab.results (parsed JSON) and on the
// console, and window.ragdollLab.done becomes true (body[data-done="1"]).

import init, { Lab } from "./pkg/ragdoll_lab.js";

const $ = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);
const headless = params.get("headless") === "1";
window.ragdollLab = { done: false, results: null };

const SPEC_KEYS = ["hits", "ground", "body", "scale", "armour", "armor", "dt", "range", "limbs", "armed", "attacking", "tune", "expect", "name"];

function finish(text) {
  $("results").textContent = text;
  let parsed = null;
  try { parsed = JSON.parse(text); } catch (_) { parsed = { error: text }; }
  window.ragdollLab.results = parsed;
  window.ragdollLab.done = true;
  document.body.dataset.done = "1";
  console.log("ragdoll-lab results", text);
}

await init();
const lab = new Lab();

// --- Tuning from the URL (the session's own; a scenario's tune= is its own). -------------------
function applyTune(text) {
  if (!text) return;
  for (const kv of text.split(",")) {
    const [k, v] = kv.split(":");
    if (k && v !== undefined) lab.set_value(k, parseFloat(v));
  }
}

// --- Headless runs. ----------------------------------------------------------------------------
const scenarioParam = params.get("scenario");
const custom = SPEC_KEYS.some((k) => params.has(k)) && params.has("hits");
if (headless || scenarioParam || custom) {
  if (scenarioParam === "all") {
    applyTune(params.get("tune"));
    finish(lab.run_all());
  } else if (scenarioParam && !custom) {
    applyTune(params.get("tune"));
    finish(lab.run_scenario(scenarioParam));
  } else if (custom) {
    const text = [...params.entries()]
      .filter(([k]) => SPEC_KEYS.includes(k))
      .map(([k, v]) => `${k}=${v}`).join("&");
    finish(lab.run_scenario(text));
  } else {
    finish(JSON.stringify({ error: "headless=1 needs scenario=<name|all> or hits=..." }));
  }
}

if (!headless) {
  applyTune(params.get("tune"));
  startPage();
}

function startPage() {
  const canvas = $("canvas");
  const ctx = canvas.getContext("2d");
  const css = getComputedStyle(document.documentElement);
  let palette = {};
  const readPalette = () => {
    const v = (n) => css.getPropertyValue(n).trim();
    palette = {
      skyTop: v("--sky-top"), skyBottom: v("--sky-bottom"),
      colours: [v("--ground-a"), v("--ground-b"), v("--bone"), v("--struck"), v("--ragdoll"), v("--arrow"), v("--foot"), v("--weapon")],
      edge: v("--bone-edge"),
    };
  };
  readPalette();
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => setTimeout(readPalette, 0));

  const cam = { yaw: 0.45, pitch: 0.22, distance: 330 };
  let playing = true;
  let speed = 1;
  let last = performance.now();

  // --- Weapons and target. ----------------------------------------------------------------------
  const W = JSON.parse(Lab.weapons_json());
  const fill = (sel, items, label = (x) => x.name, value = (x) => x.id) => {
    sel.innerHTML = "";
    for (const it of items) {
      const o = document.createElement("option");
      o.value = value(it); o.textContent = label(it);
      sel.append(o);
    }
  };
  fill($("gun"), W.guns, (g) => g.pellets > 1 ? `${g.name}, ${g.pellets} pellets` : g.name);
  fill($("load"), W.loads, (l) => ({ standard: "Standard", hp: "Hollow point", ap: "Armour piercing", jsp: "JSP", surplus: "Surplus", handload: "Hand load", match: "Match", explosive: "Explosive", slug: "Slug", magnum: "Magnum" }[l] || l), (l) => l);
  fill($("melee"), W.melee);
  fill($("blast"), W.blasts, (b) => `${b.name} (${b.damage} damage)`);
  $("gun").value = "9mm";
  fill($("ground"), JSON.parse(Lab.grounds_json()), (g) => g[0].toUpperCase() + g.slice(1), (g) => g);

  let kind = "gun";
  const showKind = () => {
    for (const b of document.querySelectorAll(".tabs button")) b.classList.toggle("on", b.dataset.kind === kind);
    for (const k of ["gun", "melee", "blast"]) {
      for (const el of document.querySelectorAll(`.kind-${k}`)) el.dataset.hidden = k === kind ? "0" : "1";
    }
  };
  const setWeapon = () => {
    const id = kind === "gun" ? $("gun").value : kind === "melee" ? $("melee").value : $("blast").value;
    const option = kind === "gun" ? $("load").value : ($("power").checked ? "power" : "");
    lab.set_weapon(kind, id, option, +$("strength").value, +$("skill").value, +$("range").value);
  };
  for (const b of document.querySelectorAll(".tabs button")) {
    b.addEventListener("click", () => { kind = b.dataset.kind; showKind(); setWeapon(); });
  }
  const outputs = { range: (v) => `${v} m`, strength: (v) => v, skill: (v) => v, scale: (v) => (+v).toFixed(2), armour: (v) => v };
  for (const [id, f] of Object.entries(outputs)) {
    $(id).addEventListener("input", () => { $(`${id}-out`).textContent = f($(id).value); });
  }
  for (const id of ["gun", "load", "melee", "blast", "power", "strength", "skill", "range"]) {
    $(id).addEventListener("change", setWeapon);
    $(id).addEventListener("input", setWeapon);
  }
  showKind();
  setWeapon();

  const setBody = () => lab.set_body($("body").value, +$("scale").value, +$("armour").value);
  for (const id of ["body", "scale", "armour"]) $(id).addEventListener("change", setBody);
  $("ground").addEventListener("change", () => lab.set_ground($("ground").value));
  const setStance = () => {
    if ($("attacking").checked) $("armed").checked = true;
    lab.set_stance($("armed").checked, $("attacking").checked);
  };
  $("armed").addEventListener("change", () => { if (!$("armed").checked) $("attacking").checked = false; setStance(); });
  $("attacking").addEventListener("change", setStance);
  const limbs = [...document.querySelectorAll("[data-limb]")];
  for (const l of limbs) {
    l.addEventListener("change", () => lab.set_limbs(limbs[0].checked, limbs[1].checked, limbs[2].checked, limbs[3].checked));
  }

  // --- Playback. --------------------------------------------------------------------------------
  const setPlaying = (p) => { playing = p; $("play").textContent = p ? "Pause" : "Play"; };
  $("play").addEventListener("click", () => setPlaying(!playing));
  $("step").addEventListener("click", () => { setPlaying(false); lab.frame_step(); });
  $("speed").addEventListener("change", () => { speed = +$("speed").value; });
  $("reset").addEventListener("click", () => lab.reset());
  addEventListener("keydown", (e) => {
    if (e.target.closest("input, textarea, select")) return;
    if (e.key === " ") { setPlaying(!playing); e.preventDefault(); }
    else if (e.key === ".") { setPlaying(false); lab.frame_step(); }
    else if (e.key === "r" || e.key === "R") lab.reset();
  });

  // --- Tuning sliders. --------------------------------------------------------------------------
  const fieldsBox = $("fields");
  const controls = new Map();
  const buildFields = () => {
    fieldsBox.innerHTML = "";
    controls.clear();
    const fields = JSON.parse(lab.fields_json());
    const groups = new Map();
    for (const f of fields) {
      if (!groups.has(f.group)) {
        const g = document.createElement("details");
        g.className = "group";
        if (f.group === "Game settings") g.open = true;
        const s = document.createElement("summary");
        s.textContent = f.group;
        g.append(s);
        fieldsBox.append(g);
        groups.set(f.group, g);
      }
      const row = document.createElement("div");
      row.className = "field";
      row.dataset.search = `${f.name} ${f.ini || ""} ${f.doc} ${f.group}`.toLowerCase();
      const top = document.createElement("div");
      top.className = "top";
      const name = document.createElement("span");
      name.className = "name";
      name.textContent = f.ini ? `${f.ini}` : f.name;
      name.title = `Game value ${f.game}. Double-click to put it back.`;
      const num = document.createElement("input");
      num.type = "number"; num.min = f.min; num.max = f.max; num.step = f.step; num.value = f.value;
      num.setAttribute("aria-label", f.name);
      top.append(name, num);
      const slider = document.createElement("input");
      slider.type = "range"; slider.min = f.min; slider.max = f.max; slider.step = f.step; slider.value = f.value;
      slider.setAttribute("aria-label", f.name);
      const doc = document.createElement("div");
      doc.className = "doc";
      doc.textContent = f.doc;
      row.append(top, slider, doc);
      groups.get(f.group).append(row);
      const set = (v) => {
        const now = lab.set_value(f.name, +v);
        slider.value = now; num.value = +now.toFixed(4);
        name.classList.toggle("changed", Math.abs(now - f.game) > 1e-6);
      };
      slider.addEventListener("input", () => set(slider.value));
      num.addEventListener("change", () => set(num.value));
      name.addEventListener("dblclick", () => set(f.game));
      controls.set(f.name, { set, f, name, slider, num });
      name.classList.toggle("changed", Math.abs(f.value - f.game) > 1e-6);
    }
    applyFilter();
  };
  const applyFilter = () => {
    const q = $("filter").value.trim().toLowerCase();
    for (const g of fieldsBox.querySelectorAll(".group")) {
      let any = false;
      for (const row of g.querySelectorAll(".field")) {
        const show = !q || row.dataset.search.includes(q);
        row.hidden = !show;
        any ||= show;
      }
      g.hidden = !any;
      if (q && any) g.open = true;
    }
  };
  $("filter").addEventListener("input", applyFilter);
  $("game").addEventListener("click", () => { lab.game_values(); buildFields(); });
  $("export").addEventListener("click", () => {
    $("ini").value = lab.export_ini();
    $("ini").select();
    navigator.clipboard?.writeText($("ini").value).catch(() => {});
  });
  $("import").addEventListener("click", () => {
    const unknown = lab.import_ini($("ini").value);
    buildFields();
    log(unknown ? `import: not understood: ${unknown}` : "import: all values read");
  });
  buildFields();

  // --- Scenarios. -------------------------------------------------------------------------------
  const scen = JSON.parse(Lab.scenarios_json());
  fill($("scenario"), scen, (s) => `${s.name} (${s.expect})`, (s) => s.name);
  const show = (text) => {
    try {
      const r = JSON.parse(text);
      const list = Array.isArray(r) ? r : [r];
      $("results").textContent = list.map((x) => x.error ? `error: ${x.error}` :
        `${x.passed ? "PASS" : "FAIL"} ${x.name}: ${x.fell ? `fell (${x.fall})` : "stayed upright"}, ${x.steps} step(s), ` +
        `${x.distance_m.toFixed(2)} m, recovery ${x.recovery_s ?? "-"} s, peak ${x.peak_speed_mps.toFixed(2)} / topple ${x.topple_mps.toFixed(2)} m/s` +
        (x.braced ? ", braced" : "") + (x.armed ? `, aim drift ${x.aim_drift_deg.toFixed(1)}°` : "") +
        (x.interrupted ? ", attack interrupted" : "") + `, arm speed ${Math.max(x.arm_spin, x.ragdoll_arm_spin).toFixed(1)} rad/s`).join("\n");
    } catch (_) { $("results").textContent = text; }
    buildFields();
    $("ground").value = lab.ground();
  };
  $("run").addEventListener("click", () => show(lab.run_scenario($("scenario").value)));
  $("run-all").addEventListener("click", () => show(lab.run_all()));
  $("run-custom").addEventListener("click", () => show(lab.run_scenario($("custom").value)));
  $("link").addEventListener("click", () => {
    const text = $("custom").value.trim() || `scenario=${$("scenario").value}`;
    const url = `${location.origin}${location.pathname}?${text}`;
    navigator.clipboard?.writeText(url).catch(() => {});
    log(`link: ${url}`);
  });

  // --- Log and status. --------------------------------------------------------------------------
  const logBox = $("log");
  const log = (line) => {
    logBox.textContent += (logBox.textContent ? "\n" : "") + line;
    const lines = logBox.textContent.split("\n");
    if (lines.length > 300) logBox.textContent = lines.slice(-300).join("\n");
    logBox.scrollTop = logBox.scrollHeight;
  };
  const statusBox = $("status");
  const updateStatus = () => {
    const s = JSON.parse(lab.status_json());
    $("state").textContent = `${s.state} · ${s.time.toFixed(2)} s${playing ? "" : " · paused"}${speed !== 1 ? ` · ${speed}x` : ""}`;
    const rows = [
      ["Hits", s.hits],
      ["Outcome", s.fell ? `fell (${s.fall})` : s.hits ? "upright" : "-"],
      ["Steps", s.steps + (s.braced ? " · braced" : "")],
      ["Moved", `${s.distance_m.toFixed(2)} m`],
      ["Recovery", s.recovery_s === null ? "-" : `${s.recovery_s.toFixed(2)} s`],
      ["Peak speed", `${s.peak_speed.toFixed(2)} m/s`],
      ["Topples at", `${s.topple.toFixed(2)} m/s`],
      ["Stack now", `${s.stack.toFixed(1)} N s`],
      ["Mass", `${s.mass.toFixed(0)} kg`],
      ["Arm speed", `${s.arm_spin.toFixed(1)} rad/s max`],
    ];
    if (s.armed) rows.push(["Aim drift", `${s.aim_drift.toFixed(1)}°`]);
    if (s.attacking || s.interrupted) rows.push(["Attack", s.interrupted ? "interrupted" : "going on"]);
    statusBox.innerHTML = rows.map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`).join("");
  };

  // --- The view. --------------------------------------------------------------------------------
  let width = 0, height = 0, ratio = 1;
  const resize = () => {
    const r = canvas.getBoundingClientRect();
    ratio = Math.min(devicePixelRatio || 1, 2);
    width = r.width; height = r.height;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
  };
  new ResizeObserver(resize).observe(canvas);
  resize();

  const draw = () => {
    lab.set_camera(cam.yaw, cam.pitch, cam.distance, width, height);
    const d = lab.draw();
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    const sky = ctx.createLinearGradient(0, 0, 0, height);
    sky.addColorStop(0, palette.skyTop); sky.addColorStop(1, palette.skyBottom);
    ctx.fillStyle = sky;
    ctx.fillRect(0, 0, width, height);
    const shade = (hex, k) => {
      const n = parseInt(hex.slice(1), 16);
      const c = (x) => Math.max(0, Math.min(255, Math.round(x * k)));
      return `rgb(${c(n >> 16)},${c((n >> 8) & 255)},${c(n & 255)})`;
    };
    for (let i = 0; i < d.length; i += 12) {
      const kindN = d[i], col = palette.colours[d[i + 11]] || "#888", s = d[i + 10];
      if (kindN === 0) {
        ctx.beginPath();
        ctx.moveTo(d[i + 2], d[i + 3]); ctx.lineTo(d[i + 4], d[i + 5]);
        ctx.lineTo(d[i + 6], d[i + 7]); ctx.lineTo(d[i + 8], d[i + 9]);
        ctx.closePath();
        ctx.fillStyle = shade(col, s);
        ctx.fill();
        ctx.strokeStyle = ctx.fillStyle; ctx.lineWidth = 0.6; ctx.stroke();
      } else if (kindN === 1) {
        const r = (d[i + 6] + d[i + 7]) / 2;
        ctx.lineCap = "round";
        ctx.beginPath(); ctx.moveTo(d[i + 2], d[i + 3]); ctx.lineTo(d[i + 4], d[i + 5]);
        ctx.strokeStyle = palette.edge; ctx.lineWidth = 2 * r + 2; ctx.stroke();
        ctx.strokeStyle = shade(col, s); ctx.lineWidth = 2 * r; ctx.stroke();
      } else {
        ctx.lineCap = "round";
        ctx.beginPath(); ctx.moveTo(d[i + 2], d[i + 3]); ctx.lineTo(d[i + 4], d[i + 5]);
        ctx.strokeStyle = col; ctx.lineWidth = d[i + 6]; ctx.stroke();
      }
    }
  };

  // Pointer: a click fires, a drag turns the view, the wheel zooms.
  let down = null;
  canvas.addEventListener("pointerdown", (e) => {
    down = { x: e.clientX, y: e.clientY, yaw: cam.yaw, pitch: cam.pitch, moved: false };
    canvas.setPointerCapture(e.pointerId);
  });
  canvas.addEventListener("pointermove", (e) => {
    const r = canvas.getBoundingClientRect();
    if (down) {
      const dx = e.clientX - down.x, dy = e.clientY - down.y;
      if (Math.hypot(dx, dy) > 4) down.moved = true;
      if (down.moved) {
        cam.yaw = down.yaw - dx * 0.008;
        cam.pitch = Math.max(-0.2, Math.min(1.4, down.pitch + dy * 0.006));
      }
    } else {
      $("hover").textContent = lab.hover(e.clientX - r.left, e.clientY - r.top);
    }
  });
  canvas.addEventListener("pointerup", (e) => {
    if (down && !down.moved) {
      const r = canvas.getBoundingClientRect();
      const what = lab.click(e.clientX - r.left, e.clientY - r.top);
      if (what === "missed") $("hover").textContent = "missed: click a body part";
    }
    down = null;
  });
  canvas.addEventListener("wheel", (e) => {
    cam.distance = Math.max(80, Math.min(1500, cam.distance * Math.exp(e.deltaY * 0.001)));
    e.preventDefault();
  }, { passive: false });

  let statusTick = 0;
  const frame = (now) => {
    const dt = Math.min(0.1, (now - last) / 1000);
    last = now;
    if (playing) lab.advance(dt * speed);
    draw();
    const lines = lab.take_log();
    if (lines) log(lines);
    if ((statusTick = (statusTick + 1) % 6) === 0) updateStatus();
    requestAnimationFrame(frame);
  };
  updateStatus();
  requestAnimationFrame(frame);
}
