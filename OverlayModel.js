// Trusted global settings only. Pack declarations never receive these values or system readings.
var declarations = [
  { name: "enabled", label: "Overlay", kind: "toggle", def: 0 },
  { name: "clock", label: "Clock", kind: "toggle", def: 1 },
  { name: "date", label: "Date", kind: "toggle", def: 1 },
  { name: "battery", label: "Battery", kind: "toggle", def: 0 },
  { name: "cpu", label: "CPU usage", kind: "toggle", def: 0 },
  { name: "memory", label: "Memory usage", kind: "toggle", def: 0 },
  { name: "position", label: "Position", kind: "range", def: 8, min: 0, max: 8, step: 1, unit: "" },
  { name: "size", label: "Size", kind: "choice", def: 1, choices: ["Small", "Medium", "Large"], values: [0, 1, 2] },
  { name: "opacity", label: "Opacity", kind: "range", def: 0.8, min: 0.2, max: 1, step: 0.1, unit: "" },
  { name: "color", label: "Colors", kind: "choice", def: 0, choices: ["Theme", "Light", "Dark"], values: [0, 1, 2] },
  { name: "drift", label: "Slow drift", kind: "toggle", def: 1 }
]
var positions = ["Top left", "Top center", "Top right", "Middle left", "Center", "Middle right", "Bottom left", "Bottom center", "Bottom right"]

function values(saved) {
  var result = {}
  saved = saved && typeof saved === "object" ? saved : {}
  for (var i = 0; i < declarations.length; i++) {
    var d = declarations[i], v = saved[d.name]
    if (typeof v !== "number" || !isFinite(v)) v = d.def
    if (d.kind === "toggle") v = v === 1 ? 1 : 0
    else if (d.kind === "choice") v = d.values.indexOf(v) >= 0 ? v : d.def
    else v = v < d.min || v > d.max ? d.def : Math.min(d.max, d.min + Math.round((v - d.min) / d.step) * d.step)
    result[d.name] = Math.round(v * 1000) / 1000
  }
  return result
}

function argument(saved) {
  var v = values(saved)
  return declarations.map(function(d) { return d.name + "=" + v[d.name] }).join(",")
}

function changed(saved, name, value) {
  if (!declarations.some(function(d) { return d.name === name })) return values(saved)
  var next = values(saved)
  next[name] = Number(value)
  return values(next)
}

// Only anonymous numeric aggregates are accepted. CPU guest counters aren't counted twice.
function reading(raw, previous) {
  if (!raw || typeof raw !== "object") return null
  for (var key of ["total", "idle", "memory"]) {
    if (typeof raw[key] !== "number" || !isFinite(raw[key]) || raw[key] < 0) return null
  }
  if (raw.total > Number.MAX_SAFE_INTEGER || raw.idle > raw.total || raw.memory > 100) return null
  var elapsed = previous ? raw.total - previous.total : 0
  var idle = previous ? raw.idle - previous.idle : 0
  var cpu = elapsed > 0 && idle >= 0 && idle <= elapsed ? Math.max(0, Math.min(100, 100 * (1 - idle / elapsed))) : -1
  return { total: raw.total, idle: raw.idle, cpu: cpu, memory: raw.memory }
}
