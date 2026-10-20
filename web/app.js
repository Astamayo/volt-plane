const ui = {
  rev: 0,
  snap: null,
  selected: sessionStorage.getItem("vp-inst") || "smu-2450",
  channel: "A",
  view: "bench",
  terms: {},
  termFor: "",
  panelKey: "",
  listKey: "",
  busKey: "",
  barKey: "",
  seqKey: "",
};

const $ = (id) => document.getElementById(id);

function esc(value) {
  return String(value).replace(/[&<>"']/g, (ch) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#39;",
  }[ch]));
}

function busLabel(id) {
  return { gpib: "GPIB", visa: "VISA", can: "CAN", lan: "LAN", usb: "USB" }[id] || String(id).toUpperCase();
}

function fmtTime(ms) {
  return new Date(ms).toLocaleTimeString([], { hour12: false });
}

function fmtV(value) {
  if (!Number.isFinite(value)) return "—";
  const digits = Math.abs(value) >= 20 ? 3 : 4;
  return `${value.toFixed(digits)} V`;
}

function fmtA(value) {
  if (!Number.isFinite(value)) return "—";
  const sign = value < 0 ? "-" : "";
  const abs = Math.abs(value);
  if (abs >= 1) return `${sign}${abs.toFixed(4)} A`;
  if (abs >= 1e-3) return `${sign}${(abs * 1e3).toFixed(3)} mA`;
  if (abs >= 1e-6) return `${sign}${(abs * 1e6).toFixed(1)} µA`;
  return `${sign}${(abs * 1e9).toFixed(1)} nA`;
}

function fmtOhms(value) {
  if (!Number.isFinite(value)) return "—";
  const abs = Math.abs(value);
  if (abs >= 1e6) return `${(value / 1e6).toFixed(3)} MΩ`;
  if (abs >= 1e3) return `${(value / 1e3).toFixed(3)} kΩ`;
  return `${value.toFixed(2)} Ω`;
}

function toast(message) {
  const el = $("toast");
  el.textContent = message;
  el.hidden = false;
  clearTimeout(toast._t);
  toast._t = setTimeout(() => { el.hidden = true; }, 3400);
}

function selected() {
  if (!ui.snap) return null;
  return ui.snap.instruments.find((inst) => inst.id === ui.selected) || ui.snap.instruments[0] || null;
}

function currentChannel(inst) {
  if (!inst || inst.channels.length === 0) return null;
  return inst.channels.find((ch) => ch.name === ui.channel) || inst.channels[0];
}

async function post(url, body) {
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
    body: JSON.stringify(body ?? {}),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.message || `Request failed (${res.status})`);
  return data;
}

function apply(snap) {
  if (!snap || typeof snap.rev !== "number" || snap.rev < ui.rev) return;
  ui.rev = snap.rev;
  ui.snap = snap;
  render();
}

async function act(url, body) {
  const data = await post(url, body);
  apply(data.snapshot || data);
  return data;
}

function setView(name) {
  ui.view = name;
  for (const id of ["bench", "buses", "sequence"]) {
    $(`view-${id}`).hidden = id !== name;
    document.querySelector(`[data-view="${id}"]`).classList.toggle("active", id === name);
  }
}

function setLink(up) {
  const el = $("link");
  el.textContent = up ? "live" : "reconnecting";
  el.classList.toggle("up", up);
}

function dotClass(inst) {
  if (inst.output_on) return "live";
  if (inst.connected) return "ok";
  return "";
}

function pillText(inst) {
  if (inst.output_on) return "LIVE";
  if (inst.connected) return "ONLINE";
  return "OFFLINE";
}

function render() {
  const snap = ui.snap;
  if (!snap) return;
  $("hal").textContent = snap.station.hal;
  $("sim").textContent = snap.station.simulation ? "Simulation" : "Hardware";
  $("station").textContent = `${snap.station.station_id} · ${snap.station.operator_id}`;
  $("live").hidden = !snap.instruments.some((inst) => inst.output_on);
  $("seq-abort").disabled = snap.run.status !== "running";
  $("scan-all").disabled = snap.buses.some((bus) => bus.state === "scanning");
  renderBusbar(snap.buses);
  renderList(snap.instruments);
  renderPanel();
  renderTerm();
  renderLog(snap.log);
  renderBuses(snap.buses);
  renderSequences(snap);
}

function renderBusbar(buses) {
  const el = $("busbar");
  const key = buses.map((bus) => bus.id).join();
  if (el.dataset.key !== key) {
    el.dataset.key = key;
    el.innerHTML = buses.map((bus) => `
      <button type="button" class="chip" data-bus="${esc(bus.id)}">
        <b><i></i>${esc(busLabel(bus.id))}</b>
        <span class="chip-detail"></span>
      </button>`).join("");
  }
  buses.forEach((bus) => {
    const chip = el.querySelector(`[data-bus="${bus.id}"]`);
    if (!chip) return;
    chip.classList.toggle("scanning", bus.state === "scanning");
    chip.classList.toggle("down", bus.state === "down");
    chip.querySelector(".chip-detail").textContent = `${bus.resources.length} resources · ${bus.detail}`;
  });
}

function renderList(instruments) {
  const el = $("list");
  const key = instruments.map((inst) => inst.id).join();
  if (el.dataset.key !== key) {
    el.dataset.key = key;
    el.innerHTML = instruments.map((inst) => `
      <div class="inst" data-id="${esc(inst.id)}" role="button" tabindex="0">
        <i class="dot"></i>
        <span><span class="name"></span><span class="sub"></span></span>
        <span class="pill"></span>
      </div>`).join("");
  }
  const current = selected();
  for (const inst of instruments) {
    const row = el.querySelector(`[data-id="${inst.id}"]`);
    if (!row) continue;
    row.classList.toggle("selected", current && inst.id === current.id);
    row.setAttribute("aria-pressed", current && inst.id === current.id ? "true" : "false");
    row.setAttribute("aria-label", `${inst.name}, ${pillText(inst)}`);
    row.querySelector(".dot").className = `dot ${dotClass(inst)}`;
    row.querySelector(".name").textContent = inst.name;
    row.querySelector(".sub").textContent = `${inst.model} · ${busLabel(inst.bus)}`;
    const pill = row.querySelector(".pill");
    pill.textContent = pillText(inst);
    pill.className = `pill ${dotClass(inst)}`;
  }
}

function panelSignature(inst) {
  if (!inst) return "none";
  const ch = currentChannel(inst);
  return [
    inst.id,
    inst.kind,
    ch ? ch.name : "",
    inst.connected,
    inst.channels.length,
    inst.lines.map((line) => line.name).join(),
  ].join("|");
}

function renderPanel() {
  const inst = selected();
  const root = $("panel");
  if (!inst) {
    root.innerHTML = `<p class="empty">No instruments on the bench.</p>`;
    ui.panelKey = "none";
    return;
  }
  if (!inst.channels.some((ch) => ch.name === ui.channel) && inst.channels[0]) {
    ui.channel = inst.channels[0].name;
  }
  const key = panelSignature(inst);
  const editing = root.contains(document.activeElement)
    && ["INPUT", "SELECT"].includes(document.activeElement.tagName);
  if (ui.panelKey !== key || !editing && root.querySelector(".panel-body") == null) {
    if (ui.panelKey !== key) {
      ui.panelKey = key;
      root.innerHTML = panelMarkup(inst);
    }
  }
  softPanel(inst);
}

function panelMarkup(inst) {
  const ch = currentChannel(inst);
  const connect = inst.connected
    ? `<button type="button" class="mini" data-act="disconnect">Disconnect</button>`
    : `<button type="button" class="mini" data-act="connect">Connect</button>`;
  let body = "";
  if (inst.kind === "smu" || inst.kind === "psu") body = sourceMarkup(inst, ch);
  else if (inst.kind === "dmm") body = meterMarkup("Voltage", "Resistance", "Resistance");
  else if (inst.kind === "can") body = canMarkup(inst);
  else if (inst.kind === "gpio") body = gpioMarkup(inst);
  return `
    <div class="panel-body">
      <div class="panel-head">
        <div>
          <p class="kicker">${esc(inst.manufacturer)}</p>
          <h2>${esc(inst.model)}</h2>
          <p class="meta">${esc(inst.resource)} · ${esc(busLabel(inst.bus))} · <span id="inst-rtt"></span></p>
          <p class="meta">${esc(inst.serial)} · firmware ${esc(inst.firmware)}</p>
        </div>
        <div>${connect}</div>
      </div>
      <p id="inst-err" class="err" hidden></p>
      ${body}
    </div>`;
}

function meterMarkup(left, right, traceLabel) {
  return `
    <div class="meters">
      <div class="meter"><span>${esc(left)}</span><b id="rd-v">—</b></div>
      <div class="meter"><span id="rd-mid-label">Current</span><b id="rd-i">—</b><span id="lim-flag" class="bad" hidden>Compliance</span></div>
      <div class="meter"><span>${esc(right)}</span><b id="rd-r">—</b></div>
    </div>
    <div class="sparks">
      <div><span class="spark-label">${esc(left)}</span><canvas id="spark-v"></canvas></div>
      <div><span class="spark-label">${esc(traceLabel)}</span><canvas id="spark-i"></canvas></div>
    </div>`;
}

function sourceMarkup(inst, ch) {
  const tabs = inst.channels.length > 1
    ? `<div class="tabs-ch">${inst.channels.map((item) => `<button type="button" data-act="ch" data-ch="${esc(item.name)}" class="${item.name === ch.name ? "on" : ""}">${esc(item.name)}</button>`).join("")}</div>`
    : "";
  const modes = inst.kind === "psu"
    ? `<option value="voltage">Voltage</option>`
    : `<option value="voltage" ${ch.source_mode === "voltage" ? "selected" : ""}>Voltage</option>
       <option value="current" ${ch.source_mode === "current" ? "selected" : ""}>Current</option>`;
  const compLabel = inst.kind === "psu" ? "Current limit" : "Compliance";
  return `
    ${tabs}
    ${meterMarkup("Voltage", "Resistance", "Current")}
    <span id="live-flag" class="live-flag" hidden>Output live</span>
    <p id="setpoint" class="setpoint"></p>
    <div class="source">
      <label class="field">Source
        <select id="src-mode">${modes}</select>
      </label>
      <label class="field">Level
        <input id="src-level" inputmode="decimal" value="${ch.source_level}" />
      </label>
      <label class="field">${compLabel}
        <input id="src-comp" inputmode="decimal" value="${ch.compliance}" />
      </label>
      <button type="button" data-act="apply">Apply</button>
      <button type="button" id="out-btn" data-act="output">Enable output</button>
    </div>
    <p class="range" id="range-text"></p>`;
}

function canMarkup(inst) {
  return `
    <div class="meters">
      <div class="meter"><span>Bus load</span><b id="can-load">—</b></div>
      <div class="meter"><span>Node</span><b>${esc(inst.resource)}</b></div>
      <div class="meter"><span>Bit rate</span><b>500 kbit/s</b></div>
    </div>
    <div class="frames" id="can-body"></div>`;
}

function gpioMarkup(inst) {
  return `
    <div class="gpio-list">
      ${inst.lines.map((line) => `
        <div class="gpio-row">
          <strong>${esc(line.name)}</strong>
          <span>${esc(line.direction)}</span>
          <button type="button" data-act="gpio" data-line="${esc(line.name)}">Off</button>
        </div>`).join("")}
    </div>`;
}

function softPanel(inst) {
  const ch = currentChannel(inst);
  const rtt = $("inst-rtt");
  if (rtt) rtt.textContent = inst.connected ? `${inst.rtt_ms.toFixed(2)} ms` : "offline";
  const err = $("inst-err");
  if (err) {
    err.hidden = !inst.last_error;
    err.textContent = inst.last_error || "";
  }
  if (inst.kind === "can") {
    const load = $("can-load");
    if (load) load.textContent = inst.connected ? `${inst.bus_load.toFixed(1)}%` : "—";
    const body = $("can-body");
    if (body) {
      body.replaceChildren();
      for (const frame of inst.frames) {
        const row = document.createElement("div");
        row.className = "frame";
        const id = document.createElement("b");
        id.textContent = `0x${frame.id.toString(16).toUpperCase()}`;
        const data = document.createElement("span");
        data.textContent = frame.data;
        row.append(id, data);
        body.append(row);
      }
    }
    return;
  }
  if (inst.kind === "gpio") {
    for (const line of inst.lines) {
      const btn = document.querySelector(`[data-line="${CSS.escape(line.name)}"]`);
      if (!btn) continue;
      btn.dataset.on = line.on ? "true" : "false";
      btn.textContent = line.on ? "High" : "Low";
      btn.classList.toggle("on", line.on);
      btn.disabled = !inst.connected;
    }
    return;
  }
  if (!ch) return;
  const rdV = $("rd-v");
  if (rdV) rdV.textContent = fmtV(ch.voltage);
  const rdI = $("rd-i");
  if (rdI) rdI.textContent = inst.kind === "dmm" ? "—" : fmtA(ch.current);
  const rdR = $("rd-r");
  if (rdR) rdR.textContent = fmtOhms(ch.resistance);
  const lim = $("lim-flag");
  if (lim) lim.hidden = !ch.limited;
  const flag = $("live-flag");
  if (flag) flag.hidden = !ch.output_on;
  const setpoint = $("setpoint");
  if (setpoint) setpoint.textContent = setpointText(inst, ch);
  const range = $("range-text");
  if (range) {
    const fixture = ch.load_ohms > 0 ? ` · fixture ${fmtOhms(ch.load_ohms)}` : "";
    range.textContent = `Range ±${inst.max_volts} V, ${fmtA(inst.max_amps)}${fixture}. Simulated resistive load.`;
  }
  const out = $("out-btn");
  if (out) {
    out.dataset.enabled = ch.output_on ? "true" : "false";
    out.textContent = ch.output_on ? "Disable output" : "Enable output";
    out.classList.toggle("live", ch.output_on);
    out.disabled = !inst.connected;
  }
  drawSpark($("spark-v"), ch.history_v, "#ffb000");
  drawSpark($("spark-i"), ch.history_i, inst.kind === "dmm" ? "#9ec0ff" : "#8ee08a");
}

function setpointText(inst, ch) {
  const level = ch.source_mode === "current" ? fmtA(ch.source_level) : fmtV(ch.source_level);
  const comp = ch.source_mode === "current" ? fmtV(ch.compliance) : fmtA(ch.compliance);
  const word = inst.kind === "psu" ? "limit" : "compliance";
  return `Set ${level} · ${word} ${comp}`;
}

function drawSpark(canvas, values, color) {
  if (!canvas) return;
  const ctx = canvas.getContext("2d");
  const width = canvas.width = Math.max(1, canvas.clientWidth * devicePixelRatio);
  const height = canvas.height = Math.max(1, canvas.clientHeight * devicePixelRatio);
  ctx.clearRect(0, 0, width, height);
  if (!values || values.length < 2) return;
  let min = Math.min(...values);
  let max = Math.max(...values);
  const mid = (min + max) / 2;
  const floor = Math.max(Math.abs(mid) * 0.01, 1e-4);
  if (max - min < floor) {
    min = mid - floor / 2;
    max = mid + floor / 2;
  }
  ctx.beginPath();
  values.forEach((value, index) => {
    const x = (index / (values.length - 1)) * (width - 2) + 1;
    const y = height - ((value - min) / (max - min)) * (height - 4) - 2;
    if (index === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  });
  ctx.strokeStyle = color;
  ctx.lineWidth = 1.6 * devicePixelRatio;
  ctx.stroke();
}

function renderTerm() {
  const inst = selected();
  const id = inst ? inst.id : "";
  $("term-prompt").textContent = id || "—";
  const input = $("term-input");
  input.disabled = !inst || !inst.connected;
  input.placeholder = !inst || !inst.connected
    ? "Connect an instrument to open a session"
    : "SCPI command";
  if (ui.termFor === id) return;
  ui.termFor = id;
  const log = $("term-log");
  log.replaceChildren();
  for (const row of ui.terms[id] || []) appendTermRow(row, false);
  log.scrollTop = log.scrollHeight;
}

function pushTerm(id, kind, text) {
  ui.terms[id] = ui.terms[id] || [];
  const row = { kind, text };
  ui.terms[id].push(row);
  if (ui.terms[id].length > 80) ui.terms[id].shift();
  if (ui.termFor === id) appendTermRow(row, true);
}

function appendTermRow(row, scroll) {
  const log = $("term-log");
  const el = document.createElement("div");
  el.className = row.kind;
  const prefix = row.kind === "out" ? "» " : row.kind === "err" ? "! " : "« ";
  el.textContent = prefix + row.text;
  log.append(el);
  if (scroll) log.scrollTop = log.scrollHeight;
}

function renderLog(lines) {
  const el = $("log");
  const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  const top = el.scrollTop;
  el.replaceChildren();
  for (const line of lines) {
    const row = document.createElement("div");
    row.className = `log-line ${line.level}`;
    const time = document.createElement("time");
    time.textContent = fmtTime(line.ts_ms);
    const message = document.createElement("span");
    message.textContent = line.message;
    row.append(time, message);
    el.append(row);
  }
  el.scrollTop = atBottom ? el.scrollHeight : top;
}

function renderBuses(buses) {
  const el = $("buses");
  const key = buses.map((bus) => `${bus.id}:${bus.resources.join(",")}`).join("|");
  if (el.dataset.key !== key) {
    el.dataset.key = key;
    el.innerHTML = buses.map((bus) => `
      <article class="card" data-bus-card="${esc(bus.id)}">
        <div class="panel-head">
          <div>
            <p class="kicker">Interface</p>
            <h2>${esc(busLabel(bus.id))}</h2>
            <p class="bus-detail meta"></p>
          </div>
          <button type="button" data-scan="${esc(bus.id)}">Scan</button>
        </div>
        <ul class="resources">
          ${bus.resources.map((resource) => `<li>${esc(resource)}</li>`).join("")}
        </ul>
      </article>`).join("");
  }
  for (const bus of buses) {
    const card = el.querySelector(`[data-bus-card="${bus.id}"]`);
    if (!card) continue;
    card.classList.toggle("scanning", bus.state === "scanning");
    card.querySelector(".bus-detail").textContent = bus.detail;
    const btn = card.querySelector("[data-scan]");
    btn.disabled = bus.state === "scanning";
    btn.textContent = bus.state === "scanning" ? "Scanning…" : "Scan";
  }
}

function renderSequences(snap) {
  const signature = [
    snap.sequences.map((seq) => seq.id).join(),
    snap.run.sequence_id,
    snap.run.status,
    snap.run.detail,
    (snap.run.outcomes || []).join(),
  ].join("|");
  const el = $("sequences");
  if (el.dataset.key === signature) {
    $("seq-detail").textContent = snap.run.detail || "Run a scripted check against the simulated bench. Abort leaves cleanup steps in place.";
    return;
  }
  el.dataset.key = signature;
  $("seq-detail").textContent = snap.run.detail || "Run a scripted check against the simulated bench. Abort leaves cleanup steps in place.";
  const busy = snap.run.status === "running";
  el.innerHTML = snap.sequences.map((seq) => {
    const active = snap.run.sequence_id === seq.id;
    const outcomes = active ? snap.run.outcomes : [];
    const badge = active && snap.run.status !== "idle" ? snap.run.status : "";
    return `
      <article class="seq-card">
        <div class="panel-head">
          <div>
            <h2>${esc(seq.name)}</h2>
            <p>${esc(seq.summary)}</p>
          </div>
          <span class="badge ${esc(badge)}">${esc(badge)}</span>
        </div>
        <ol class="steps">
          ${seq.steps.map((name, index) => {
            const outcome = outcomes[index] || "pending";
            return `<li class="step ${esc(outcome)}"><i></i><span>${esc(name)}</span></li>`;
          }).join("")}
        </ol>
        <button type="button" data-run="${esc(seq.id)}" ${busy ? "disabled" : ""}>Run</button>
      </article>`;
  }).join("");
}

function selectInstrument(id) {
  ui.selected = id;
  ui.channel = "A";
  ui.panelKey = "";
  sessionStorage.setItem("vp-inst", id);
  const active = document.activeElement;
  if (active && active.blur && $("panel").contains(active)) active.blur();
  render();
}

document.querySelectorAll("[data-view]").forEach((btn) => {
  btn.addEventListener("click", () => setView(btn.dataset.view));
});

$("connect-all").addEventListener("click", async () => {
  try { await act("/api/connect-all", {}); }
  catch (error) { toast(error.message); }
});

$("out-off").addEventListener("click", async () => {
  try { await act("/api/outputs/off", {}); }
  catch (error) { toast(error.message); }
});

$("scan-all").addEventListener("click", async () => {
  try { await act("/api/scan", {}); }
  catch (error) { toast(error.message); }
});

$("seq-abort").addEventListener("click", async () => {
  try { await act("/api/sequences/abort", {}); }
  catch (error) { toast(error.message); }
});

$("busbar").addEventListener("click", (event) => {
  if (event.target.closest("[data-bus]")) setView("buses");
});

$("list").addEventListener("click", (event) => {
  const row = event.target.closest("[data-id]");
  if (row) selectInstrument(row.dataset.id);
});

$("list").addEventListener("keydown", (event) => {
  if (event.key !== "Enter" && event.key !== " ") return;
  const row = event.target.closest("[data-id]");
  if (!row) return;
  event.preventDefault();
  selectInstrument(row.dataset.id);
});

$("panel").addEventListener("click", async (event) => {
  const btn = event.target.closest("[data-act]");
  if (!btn) return;
  const inst = selected();
  if (!inst) return;
  try {
    if (btn.dataset.act === "ch") {
      ui.channel = btn.dataset.ch;
      ui.panelKey = "";
      const active = document.activeElement;
      if (active && active.blur) active.blur();
      render();
      return;
    }
    if (btn.dataset.act === "connect") {
      await act(`/api/instruments/${inst.id}/connect`, {});
    } else if (btn.dataset.act === "disconnect") {
      await act(`/api/instruments/${inst.id}/disconnect`, {});
    } else if (btn.dataset.act === "apply") {
      const mode = $("src-mode")?.value || "voltage";
      const level = Number($("src-level").value);
      const compliance = Number($("src-comp").value);
      if (!Number.isFinite(level) || !Number.isFinite(compliance)) {
        toast("Enter a source level and a compliance.");
        return;
      }
      await act(`/api/instruments/${inst.id}/source`, {
        channel: currentChannel(inst)?.name || "A",
        mode,
        level,
        compliance,
      });
    } else if (btn.dataset.act === "output") {
      await act(`/api/instruments/${inst.id}/output`, {
        channel: currentChannel(inst)?.name || "A",
        enabled: btn.dataset.enabled !== "true",
      });
    } else if (btn.dataset.act === "gpio") {
      await act(`/api/instruments/${inst.id}/gpio`, {
        line: btn.dataset.line,
        on: btn.dataset.on !== "true",
      });
    }
  } catch (error) {
    toast(error.message);
  }
});

$("panel").addEventListener("keydown", (event) => {
  if (event.key === "Enter" && event.target.matches("input")) {
    event.preventDefault();
    $("panel").querySelector("[data-act='apply']")?.click();
  }
});

$("buses").addEventListener("click", async (event) => {
  const btn = event.target.closest("[data-scan]");
  if (!btn) return;
  try { await act("/api/scan", { bus: btn.dataset.scan }); }
  catch (error) { toast(error.message); }
});

$("sequences").addEventListener("click", async (event) => {
  const btn = event.target.closest("[data-run]");
  if (!btn) return;
  setView("sequence");
  try { await act(`/api/sequences/${btn.dataset.run}/run`, {}); }
  catch (error) { toast(error.message); }
});

$("terminal").addEventListener("submit", async (event) => {
  event.preventDefault();
  const inst = selected();
  const input = $("term-input");
  const line = input.value.trim();
  if (!inst || !line) return;
  input.value = "";
  try {
    const data = await post(`/api/instruments/${inst.id}/command`, { line });
    pushTerm(inst.id, "out", line);
    pushTerm(inst.id, "in", data.response);
    apply(data.snapshot);
  } catch (error) {
    pushTerm(inst.id, "out", line);
    pushTerm(inst.id, "err", error.message);
    toast(error.message);
  }
});

function connectWs() {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  const ws = new WebSocket(`${proto}://${location.host}/api/stream`);
  ws.onopen = () => setLink(true);
  ws.onmessage = (event) => {
    try { apply(JSON.parse(event.data)); }
    catch { /* ignore malformed frames */ }
  };
  ws.onclose = () => {
    setLink(false);
    setTimeout(connectWs, 800);
  };
}

function tickClock() {
  $("clock").textContent = new Date().toLocaleTimeString([], { hour12: false });
}

tickClock();
setInterval(tickClock, 1000);
fetch("/api/snapshot")
  .then((res) => {
    if (!res.ok) throw new Error("Bench unavailable");
    return res.json();
  })
  .then(apply)
  .catch((error) => toast(error.message));
connectWs();
