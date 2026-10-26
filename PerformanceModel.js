// Local display preferences only. No pack-provided commands or paths.
var declarations = [
  { name: "enabled", label: "Prepared playback", kind: "toggle", def: 0 },
  { name: "batteryOnly", label: "Use on battery only", kind: "toggle", def: 1 },
  { name: "maxMiB", label: "Storage limit", kind: "choice", def: 512,
    choices: ["256 MB", "512 MB", "1 GB", "2 GB"], values: [256, 512, 1024, 2048] }
]
function values(saved) {
  saved = saved && typeof saved === "object" ? saved : {}
  var out = {}
  for (var i = 0; i < declarations.length; i++) {
    var d = declarations[i]
    var v = Object.prototype.hasOwnProperty.call(saved, d.name) ? saved[d.name] : d.def
    if (typeof v !== "number" || !isFinite(v)) v = d.def
    out[d.name] = d.kind === "toggle" ? (v === 1 ? 1 : 0) : (d.values.indexOf(v) >= 0 ? v : d.def)
  }
  return out
}
function changed(saved, name, value) {
  var out = values(saved)
  if (Object.prototype.hasOwnProperty.call(out, name)) out[name] = value
  return values(out)
}
