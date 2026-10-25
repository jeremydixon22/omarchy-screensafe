// Color selections stay numeric in settings and IPC. No strings become expressions.
var customBase = 16777216
function valid(v) { return typeof v === "number" && isFinite(v) && Math.floor(v) === v && ((v >= 0 && v <= 27) || (v >= customBase && v < customBase * 2)) }
function palette(list) {
  if (!Array.isArray(list) || list.length !== 27) return []
  var out = []
  for (var i = 0; i < list.length; i++) {
    var c = list[i]
    if (!c || c.value !== i + 1 || typeof c.name !== "string" || !/^[a-z_]{1,32}$/.test(c.name) || typeof c.color !== "string" || !/^#[0-9a-fA-F]{6}$/.test(c.color)) return []
    out.push({ value: c.value, name: c.name, color: c.color.toLowerCase() })
  }
  return out
}
function hex(v, swatches) {
  if (valid(v) && v >= customBase) return "#" + (v - customBase).toString(16).padStart(6, "0")
  var i = v > 0 && v <= 27 ? v - 1 : 2
  return swatches && swatches[i] ? swatches[i].color : "#7aa2f7"
}
function custom(hex) { return /^#[0-9a-fA-F]{6}$/.test(hex) ? customBase + parseInt(hex.slice(1), 16) : customBase + 0x7aa2f7 }
function name(v, swatches) { return v === 0 ? "Theme colors" : v >= customBase ? hex(v, swatches).toUpperCase() : swatches && swatches[v - 1] ? swatches[v - 1].name.replace(/_/g, " ") : "Theme color" }
function hsv(hex) {
  var rgb = parseInt(hex.slice(1), 16), r = ((rgb >> 16) & 255) / 255, g = ((rgb >> 8) & 255) / 255, b = (rgb & 255) / 255
  var max = Math.max(r, g, b), min = Math.min(r, g, b), d = max - min, h = 0
  if (d > 0) h = max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4
  return { h: h / 6, s: max === 0 ? 0 : d / max, v: max }
}
function fromHsv(h, s, v) {
  h = ((Number(h) % 1) + 1) % 1; s = Math.max(0, Math.min(1, Number(s))); v = Math.max(0, Math.min(1, Number(v)))
  if (!isFinite(h) || !isFinite(s) || !isFinite(v)) return customBase
  var sector = h * 6, f = sector - Math.floor(sector), p = v * (1 - s), q = v * (1 - s * f), t = v * (1 - s * (1 - f))
  var rgb = [[v,t,p],[q,v,p],[p,v,t],[p,q,v],[t,p,v],[v,p,q]][Math.floor(sector) % 6]
  return customBase + (Math.round(rgb[0] * 255) << 16) + (Math.round(rgb[1] * 255) << 8) + Math.round(rgb[2] * 255)
}
function step(v, direction, wrap, swatches) {
  var values = [0].concat((swatches || []).map(function(c) { return c.value }))
  values.push(v >= customBase ? v : custom(hex(v, swatches)))
  var at = v >= customBase ? values.length - 1 : Math.max(0, values.indexOf(v)), next = at + direction
  return values[wrap ? (next + values.length) % values.length : Math.max(0, Math.min(values.length - 1, next))]
}
function adjust(v, channel, direction, swatches) {
  var c = hsv(hex(v, swatches))
  if (channel === "h") c.h += direction / 72
  else if (channel === "s") c.s += direction * 0.05
  else if (channel === "v") c.v += direction * 0.05
  return fromHsv(c.h, c.s, c.v)
}
function textColor(hex) {
  var rgb = parseInt(hex.slice(1), 16)
  return (((rgb >> 16) & 255) * .2126 + ((rgb >> 8) & 255) * .7152 + (rgb & 255) * .0722) > 145 ? "#101010" : "#ffffff"
}
