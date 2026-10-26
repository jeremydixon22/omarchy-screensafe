import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Services.UPower
import "OverlayModel.js" as OverlayModel
import "ColorModel.js" as ColorModel
import "PerformanceModel.js" as PerformanceModel

// Shows a ScreenSafe pack when you're idle, in place of Omarchy's screensaver.
//
// Omarchy's idle service stays in charge of everything else: it locks on time, honors stay-awake, and its lock
// closes the screensaver. While "Enable" is on, this service switches Omarchy's own screensaver off
// (and back on when it's turned off or the plugin unloads) and opens ScreenSafe one second after Omarchy would
// have opened its own, or after ScreenSafe's own idle time if one is set. Omarchy's settings are only read. The
// renderer's command line names Omarchy's screensaver class, so Omarchy treats it as the screensaver that's running:
// locking closes it and the menu's Screensaver entry won't open a second one.
//
// Every program here is a trusted bundled plugin program or Omarchy's own shell command, started by absolute path with a
// cleared environment holding only what it needs. Each short call has a deadline, and the renderer's output is
// bounded where it's produced.
Item {
  id: root

  // Injected by the shell: this plugin's scoped facade.
  property var shell: null

  readonly property string pluginId: "io.github.jeremydixon22.screensafe"
  readonly property string home: Quickshell.env("HOME") || ""
  readonly property string runtimeDir: Quickshell.env("XDG_RUNTIME_DIR") || ""
  readonly property string waylandDisplay: Quickshell.env("WAYLAND_DISPLAY") || ""
  readonly property string stateHome: Quickshell.env("XDG_STATE_HOME") || ""
  // Injected by the shell; the environment is the fallback.
  property string omarchyPath: Quickshell.env("OMARCHY_PATH") || "/usr/share/omarchy"
  readonly property string pluginDir: decodeURIComponent(String(Qt.resolvedUrl(".")).replace(/^file:\/\//, "").replace(/\/$/, ""))
  // The renderer built for this machine: bin/screensafe-x86_64 or bin/screensafe-aarch64.
  readonly property string arch: String(archFile.text() || "").trim()
  readonly property bool supported: arch === "x86_64" || arch === "aarch64"
  readonly property string renderer: pluginDir + "/bin/screensafe-" + (supported ? arch : "x86_64")
  readonly property string builtinPacks: pluginDir + "/packs"
  readonly property string userPacks: home + "/.config/omarchy/screensafe/packs"
  readonly property string themeDir: home + "/.local/state/omarchy/current/theme"
  readonly property string themePack: themeDir + "/screensafe"
  readonly property string defaultPack: builtinPacks + "/omarchy"
  readonly property string branding: home + "/.config/omarchy/branding/screensaver.txt"
  readonly property string logo: omarchyPath + "/logo.txt"
  // Someone's own badge, for packs that show one: badge.png, .webp, .jpg or .jpeg here.
  readonly property string badge: home + "/.config/omarchy/screensafe/badge"
  // Someone's own photos, for packs that show them.
  readonly property string photos: home + "/Pictures/Screensaver"
  readonly property string videos: home + "/Videos/Screensaver"
  // The shell's time zone, if it has one set, so a pack's clock shows the same time as the bar's.
  readonly property string timeZone: Quickshell.env("TZ") || ""

  // This instance's name for its claim on Omarchy's screensaver switch, so that a late release from an earlier
  // instance (after a shell reload) can't undo this one's.
  readonly property string token: "s" + Date.now().toString(36) + Math.floor(Math.random() * 1e9).toString(36)

  // Settings, inline on the plugin's entry in shell.json.
  property var entry: ({})
  property bool loaded: false
  readonly property bool onIdle: entry.onIdle === true
  readonly property string pack: typeof entry.pack === "string" && entry.pack !== "" ? entry.pack : "auto"
  // ScreenSafe's own idle time in seconds, or 0 to follow Omarchy's.
  readonly property int ownSeconds: validSeconds(entry.idleSeconds)
  // Omarchy's idle times, from its settings.
  property int screensaverSeconds: 150
  property int lockSeconds: 300
  readonly property int idleSeconds: ownSeconds > 0 ? ownSeconds : screensaverSeconds
  // The paths of the packs starred as favorites, listed first in the panel.
  readonly property var favorites: validFavorites(entry.favorites)
  // The idle times the panel offers, besides following Omarchy's.
  readonly property var idleChoices: [60, 120, 180, 300, 600, 900, 1200, 1800, 2700, 3600]

  // What the panel shows.
  property var packs: []
  property bool cataloguing: catalogProc.running
  property string problem: ""
  property bool holding: false
  readonly property bool running: rendererProc.running
  readonly property string currentPackName: nameOf(resolvedPack())

  function nameOf(path) {
    for (var i = 0; i < root.packs.length; i++)
      if (root.packs[i].path === path) return root.packs[i].name
    return ""
  }

  function themeHasPack() {
    for (var i = 0; i < root.packs.length; i++)
      if (root.packs[i].path === root.themePack && root.packs[i].ok) return true
    return false
  }

  // Whether the catalog lists a usable pack at `path`. Only such a path is ever given to the renderer: a chosen pack
  // read from shell.json that isn't one (moved, removed, or edited by hand) is shown as Automatic would be instead.
  function listed(path) {
    for (var i = 0; i < root.packs.length; i++)
      if (root.packs[i].path === path && root.packs[i].ok) return true
    return false
  }

  function resolvedPack() {
    if (root.pack !== "auto" && listed(root.pack)) return root.pack
    return themeHasPack() ? root.themePack : root.defaultPack
  }

  function environment(withShell) {
    var env = { HOME: root.home, XDG_RUNTIME_DIR: root.runtimeDir, WAYLAND_DISPLAY: root.waylandDisplay }
    if (root.stateHome !== "") env.XDG_STATE_HOME = root.stateHome
    if (/^:?[A-Za-z0-9_+\-\/,.<>]{1,64}$/.test(root.timeZone)) env.TZ = root.timeZone
    if (withShell) {
      env.OMARCHY_PATH = root.omarchyPath
      env.PATH = root.omarchyPath + "/bin:/usr/bin:/bin"
    }
    return env
  }

  function lastLine(text) {
    var lines = String(text || "").trim().split("\n")
    return lines[lines.length - 1]
  }

  // ---- settings

  function saveSettings(change) {
    var next = {}
    for (var k in root.entry) next[k] = root.entry[k]
    for (var c in change) next[c] = change[c]
    root.entry = next
    if (root.shell && typeof root.shell.updateEntryInline === "function")
      root.shell.updateEntryInline(root.pluginId, next)
  }

  function setOnIdle(value) {
    if (!!value) root.problem = ""
    saveSettings({ onIdle: !!value })
    return root.onIdle ? "on" : "off"
  }

  function validSeconds(value) {
    var n = Number(value)
    return isFinite(n) && n >= 10 && n <= 86400 ? Math.floor(n) : 0
  }

  // seconds: from 10 to 86400, or 0 (or "omarchy") to follow Omarchy's idle time.
  function setIdleSeconds(seconds) {
    if (String(seconds) === "omarchy") seconds = 0
    var n = Number(seconds) === 0 ? 0 : validSeconds(seconds)
    if (n === 0 && Number(seconds) !== 0) return "seconds must be 0, or from 10 to 86400"
    saveSettings({ idleSeconds: n })
    return String(root.idleSeconds) + (root.ownSeconds > 0 ? " s" : " s (Omarchy's)")
  }

  // Steps through the panel's choices: 0, then each of idleChoices.
  function stepIdleSeconds(direction) {
    var list = [0].concat(root.idleChoices)
    var i = list.indexOf(root.ownSeconds)
    if (i < 0) {
      // A time set some other way: step to the nearest choice that way.
      i = 0
      while (i + 1 < list.length && list[i + 1] < root.ownSeconds) i++
      if (direction < 0) i++
    }
    i = Math.max(0, Math.min(list.length - 1, i + (direction > 0 ? 1 : -1)))
    return setIdleSeconds(list[i])
  }

  // At most 64 distinct, non-empty paths; anything else in the setting is ignored.
  function validFavorites(value) {
    var list = []
    if (!Array.isArray(value)) return list
    for (var i = 0; i < value.length && list.length < 64; i++) {
      var p = value[i]
      if (typeof p === "string" && p !== "" && p.length <= 4096 && list.indexOf(p) < 0) list.push(p)
    }
    return list
  }

  function isFavorite(path) { return root.favorites.indexOf(String(path)) >= 0 }

  function setFavorite(path, on) {
    path = String(path || "")
    if (path === "" || path === "auto") return "only a pack can be a favorite"
    var list = root.favorites.filter(function(p) { return p !== path })
    if (on) {
      if (list.length >= 64) return "at most 64 favorites"
      list.push(path)
    }
    saveSettings({ favorites: list })
    return root.isFavorite(path) ? "favorite" : "not a favorite"
  }

  function toggleFavorite(path) { return setFavorite(path, !root.isFavorite(path)) }

  // Global overlay data stays separate from pack settings and expressions. Off until opted in.
  readonly property var performanceValues: PerformanceModel.values(root.entry.performance)
  readonly property string cacheWorker: root.pluginDir + "/bin/screensafe-cache-" + root.arch
  readonly property string cacheRoot: root.home + "/.cache/omarchy/screensafe/prepared"
  property string performanceState: "unknown"
  readonly property bool preparing: cacheWorkProc.running && cacheWorkProc.action === "prepare"
  readonly property string performanceDetail: {
    if (root.preparing) return "Preparing and checking a local loop. You can cancel at any time."
    if (root.overlayValues.enabled === 1) return "Overlay is on. Normal playback is used so the combined picture remains guarded."
    if (root.performanceState === "ready") return "A prepared loop is ready for this screensaver, theme and display."
    if (root.performanceState === "empty") return "No matching loop is prepared. Normal playback is used."
    if (root.performanceState === "ineligible") return "This screensaver needs live playback."
    if (root.performanceState === "cancelled") return "Preparation cancelled. Normal playback is available."
    if (root.performanceState === "unavailable") return "Prepared playback is unavailable for this setup. Normal playback is available."
    return "Prepare a loop on external power to make it available later."
  }
  function setPerformance(name, value) {
    if (!PerformanceModel.declarations.some(function(d) { return d.name === name })) return
    saveSettings({ performance: PerformanceModel.changed(root.entry.performance, name, value) })
    if (name === "maxMiB") root.trimPrepared()
    else root.refreshPerformance()
  }
  function cacheEnvironment() {
    var env = root.environment(false)
    env.SCREENSAFE_PARENT_PID = String(Quickshell.processId)
    var signature = Quickshell.env("HYPRLAND_INSTANCE_SIGNATURE") || ""
    if (/^[A-Za-z0-9_\-]{1,200}$/.test(signature)) env.HYPRLAND_INSTANCE_SIGNATURE = signature
    return env
  }
  function cacheCommand(action, path) {
    var command = ["/usr/bin/python3", root.pluginDir + "/cache/manager.py", action,
      "--renderer", root.renderer, "--worker", root.cacheWorker, "--root", root.cacheRoot,
      "--limit", String(root.performanceValues.maxMiB), "--battery-only", String(root.performanceValues.batteryOnly),
      "--pack", path, "--theme", root.themeDir, "--branding", root.branding, "--logo", root.logo,
      "--badge", root.badge, "--photos", root.photos, "--videos", root.videos,
      "--settings", root.settingsArg(path), "--overlay", root.overlayArgument]
    if (action === "run") command.push("--managed")
    return command
  }
  function refreshPerformance() {
    if (cacheStatusProc.running || root.preparing) return
    cacheStatusProc.command = root.cacheCommand("status", root.resolvedPack())
    cacheStatusProc.environment = root.cacheEnvironment()
    root.begin(cacheStatusProc)
  }
  function prepareSelected() {
    if (cacheWorkProc.running) { cacheWorkProc.cancelled = true; cacheWorkProc.running = false; root.performanceState = "cancelled"; return }
    cacheWorkProc.action = "prepare"
    cacheWorkProc.command = root.cacheCommand("prepare", root.resolvedPack())
    cacheWorkProc.environment = root.cacheEnvironment()
    root.begin(cacheWorkProc)
  }
  property string pendingCacheAction: ""
  function clearPrepared() { root.cacheMaintenance("clear") }
  function trimPrepared() { root.cacheMaintenance("trim") }
  function cacheMaintenance(action) {
    if (cacheWorkProc.running) { root.pendingCacheAction = action; cacheWorkProc.cancelled = true; cacheWorkProc.running = false; return }
    cacheWorkProc.action = action
    cacheWorkProc.command = root.cacheCommand(action, root.resolvedPack())
    cacheWorkProc.environment = root.cacheEnvironment()
    root.begin(cacheWorkProc)
  }

  readonly property var overlayDeclarations: OverlayModel.declarations
  readonly property var overlayValues: OverlayModel.values(root.entry.overlay)
  readonly property string overlayArgument: OverlayModel.argument(root.entry.overlay)
  property var systemReading: null
  readonly property bool overlayActive: overlayValues.enabled === 1 && (rendererProc.running || livePreviewProc.running)
  function setOverlay(name, value) {
    if (!overlayDeclarations.some(function(d) { return d.name === name })) return "unknown setting"
    saveSettings({ overlay: OverlayModel.changed(root.entry.overlay, name, value) })
    return String(root.overlayValues[name])
  }
  function resetOverlay() { saveSettings({ overlay: OverlayModel.values({}) }) }
  function pushOverlay() {
    var line = "overlay " + root.overlayArgument + "\n"
    if (rendererProc.running) rendererProc.write(line)
    if (livePreviewProc.running) livePreviewProc.write(line)
    root.pushMetrics()
  }
  onOverlayArgumentChanged: root.pushOverlay()
  function pushMetrics() {
    if (!root.overlayActive) return
    var d = UPower.displayDevice
    var battery = d && d.isPresent && isFinite(d.percentage) ? Math.max(0, Math.min(100, d.percentage * 100)) : -1
    var reading = root.systemReading
    var cpu = root.overlayValues.cpu === 1 && reading ? reading.cpu : -1
    var memory = root.overlayValues.memory === 1 && reading ? reading.memory : -1
    var line = "stats cpu=" + Math.round(cpu) + ",memory=" + Math.round(memory) + ",battery=" + (root.overlayValues.battery === 1 ? Math.round(battery) : -1) + "\n"
    if (rendererProc.running) rendererProc.write(line)
    if (livePreviewProc.running) livePreviewProc.write(line)
  }
  function requestSystem() {
    if (root.overlayActive && (root.overlayValues.cpu === 1 || root.overlayValues.memory === 1))
      runShort(systemProc, ["system-info"])
    else root.systemReading = null
    root.pushMetrics()
  }
  Timer { interval: 2000; repeat: true; running: root.overlayActive; onTriggered: root.requestSystem() }
  onOverlayActiveChanged: if (root.overlayActive) Qt.callLater(root.requestSystem)

  // ---- pack settings: what a pack offers (from the catalog), and the values chosen, saved inline in shell.json by
  // pack path. Values are only ever ones the pack's own declaration allows; the renderer checks them again.

  readonly property int maxSettings: 8
  property var themeSwatches: []

  // A pack's declared settings from the catalog, kept only if well formed.
  function validDeclarations(list) {
    var out = []
    if (!Array.isArray(list)) return out
    for (var i = 0; i < list.length && out.length < root.maxSettings; i++) {
      var d = list[i]
      if (!d || typeof d.name !== "string" || !/^[a-z][a-z0-9_]{0,31}$/.test(d.name)) continue
      if (typeof d.label !== "string" || d.label === "" || d.label.length > 40) continue
      var item = { name: d.name, label: d.label, kind: d.kind, def: Number(d.default) }
      if (!isFinite(item.def)) continue
      if (d.kind === "toggle") {
        item.def = item.def !== 0 ? 1 : 0
      } else if (d.kind === "choice") {
        if (!Array.isArray(d.choices) || !Array.isArray(d.values) || d.choices.length !== d.values.length
            || d.choices.length < 2 || d.choices.length > 6) continue
        var ok = true
        for (var c = 0; c < d.values.length; c++)
          if (typeof d.choices[c] !== "string" || d.choices[c].length > 16 || !isFinite(Number(d.values[c]))) ok = false
        if (!ok) continue
        item.choices = d.choices.slice()
        item.values = d.values.map(Number)
      } else if (d.kind === "range") {
        item.min = Number(d.min); item.max = Number(d.max); item.step = Number(d.step)
        item.unit = typeof d.unit === "string" ? d.unit.slice(0, 8) : ""
        if (!isFinite(item.min) || !isFinite(item.max) || !(item.step > 0) || item.max <= item.min) continue
      } else if (d.kind === "color") {
        if (!ColorModel.valid(item.def)) continue
        if (d.when !== undefined) {
          if (typeof d.when !== "string" || !/^[a-z][a-z0-9_]{0,31}$/.test(d.when) || typeof d.whenValue !== "number" || !isFinite(d.whenValue)) continue
          item.when = d.when; item.whenValue = d.whenValue
        }
      } else {
        continue
      }
      out.push(item)
    }
    return out
  }

  function declarationsOf(path) {
    for (var i = 0; i < root.packs.length; i++) if (root.packs[i].path === path) return root.packs[i].settings || []
    return []
  }

  // A value the declaration allows: a toggle's 0 or 1, one of a choice's values, a range's value snapped in bounds.
  function allowed(d, v) {
    v = Number(v)
    if (!isFinite(v)) return d.def
    if (d.kind === "toggle") return v !== 0 ? 1 : 0
    if (d.kind === "choice") return d.values.indexOf(v) >= 0 ? v : d.def
    if (d.kind === "color") return ColorModel.valid(v) ? v : d.def
    if (v < d.min || v > d.max) return d.def
    return Math.min(d.max, Math.max(d.min, d.min + Math.round((v - d.min) / d.step) * d.step))
  }

  // The saved values for a pack, each checked against what it declares; defaults for the rest.
  function settingsOf(path) {
    var saved = root.entry.settings && typeof root.entry.settings === "object" ? root.entry.settings[path] : undefined
    var out = {}
    var decl = root.declarationsOf(path)
    for (var i = 0; i < decl.length; i++)
      out[decl[i].name] = saved && typeof saved === "object" && saved[decl[i].name] !== undefined ? root.allowed(decl[i], saved[decl[i].name]) : decl[i].def
    return out
  }

  // For the renderer: "name=value,...", in the pack's own order.
  function settingsArg(path) {
    var decl = root.declarationsOf(path)
    var values = root.settingsOf(path)
    var parts = []
    for (var i = 0; i < decl.length; i++) parts.push(decl[i].name + "=" + values[decl[i].name])
    return parts.join(",")
  }

  // Saves one setting of a listed pack. Values for packs no longer listed are dropped as it's saved.
  function setSetting(path, name, value) {
    var decl = root.declarationsOf(path)
    var d = null
    for (var i = 0; i < decl.length; i++) if (decl[i].name === name) d = decl[i]
    if (!d || !root.listed(path)) return "unknown setting"
    var all = {}
    var old = root.entry.settings && typeof root.entry.settings === "object" ? root.entry.settings : {}
    for (var p in old) if (root.listed(p) && p !== path) all[p] = old[p]
    var mine = root.settingsOf(path)
    mine[name] = root.allowed(d, value)
    // Only values that differ from the defaults are kept.
    var kept = {}
    var any = false
    for (var j = 0; j < decl.length; j++) if (mine[decl[j].name] !== decl[j].def) { kept[decl[j].name] = mine[decl[j].name]; any = true }
    if (any) all[path] = kept
    saveSettings({ settings: all })
    root.updateLive(path)
    return String(mine[name])
  }

  function resetSettings(path) {
    if (!root.listed(path)) return "unknown pack"
    var all = {}
    var old = root.entry.settings && typeof root.entry.settings === "object" ? root.entry.settings : {}
    for (var p in old) if (root.listed(p) && p !== path) all[p] = old[p]
    saveSettings({ settings: all })
    root.updateLive(path)
    return "reset"
  }

  // ---- the settings page's live preview: the renderer drawing the pack in a box laid over the page, taking no input,
  // and told each change of its settings as it runs (see the renderer's `preview` command).

  // Whether the settings page shows a live preview; saved, on unless turned off.
  readonly property bool livePreviews: root.entry.livePreview !== false
  function setLivePreviews(on) { saveSettings({ livePreview: !!on }) }

  property string livePath: ""
  // A preview waiting for the last one to finish exiting before it starts: { path, args, region }, or null. Starting
  // the next only once the last has exited keeps one preview at a time, and keeps a late exit from being mistaken
  // for the new one's.
  property var livePending: null
  property string liveRegion: ""
  // A pack whose preview failed: not started again until its settings change or the page is left, so a failing
  // preview isn't restarted over and over while its page is open.
  property string liveFailed: ""
  property bool liveStopping: false
  function startLive(path, x, y, w, h, output) {
    if (!root.supported || !root.listed(path) || path === root.liveFailed) return
    var box = [x, y, w, h].map(function(v) { return Math.round(Number(v)) })
    if (box.some(function(v) { return !isFinite(v) || v < 0 || v > 16384 }) || box[2] < 16 || box[3] < 9) return
    var region = box.join(",")
    if (livePreviewProc.running && root.livePath === path && !root.livePending) {
      // Already showing this pack: only a move needs telling, and only when it has moved.
      if (region !== root.liveRegion) {
        livePreviewProc.write("region " + region + "\n")
        root.liveRegion = region
      }
      return
    }
    if (root.livePending && root.livePending.path === path) return
    var args = ["preview", "--pack", path, "--region", region, "--theme", root.themeDir, "--branding", root.branding,
                "--logo", root.logo, "--badge", root.badge, "--photos", root.photos, "--videos", root.videos]
    if (/^[A-Za-z0-9._-]{1,64}$/.test(String(output || ""))) args = args.concat(["--output", String(output)])
    if (livePreviewProc.running) {
      root.livePending = { path: path, args: args, region: region }
      root.liveStopping = true
      livePreviewProc.write("quit\n")
      livePreviewProc.running = false
      return
    }
    root.launchLive(path, args, region)
  }
  // Starts a preview, with the pack's settings as they are at that moment.
  function launchLive(path, args, region) {
    var settings = root.settingsArg(path)
    if (settings !== "") args = args.concat(["--settings", settings])
    root.livePath = path
    root.liveRegion = region
    livePreviewProc.command = [root.renderer].concat(args, ["--overlay", root.overlayArgument])
    livePreviewProc.environment = root.environment(false)
    liveErr.reset()
    root.liveStopping = false
    livePreviewProc.running = true
  }
  function stopLive() {
    root.livePending = null
    root.livePath = ""
    root.liveRegion = ""
    root.liveFailed = ""
    if (livePreviewProc.running) {
      root.liveStopping = true
      livePreviewProc.write("quit\n")
      livePreviewProc.running = false
    }
  }
  // Tells a running live preview its pack's settings as they now are.
  function updateLive(path) {
    if (root.liveFailed === path) root.liveFailed = ""
    if (livePreviewProc.running && root.livePath === path && !root.livePending) livePreviewProc.write("settings " + root.settingsArg(path) + "\n")
  }

  // How many photos a pack has to show, or -1 if it doesn't show photos.
  function photosOf(path) {
    for (var i = 0; i < root.packs.length; i++) if (root.packs[i].path === path) return root.packs[i].photos
    return -1
  }
  function videosOf(path) {
    for (var i = 0; i < root.packs.length; i++) if (root.packs[i].path === path) return root.packs[i].videos
    return -1
  }

  // Opens the photos folder in the file manager, making it first if it isn't there. The renderer makes it, and says
  // where it is; only that exact folder is opened.
  function openPhotosFolder() {
    if (!root.supported || photosFolderProc.running || videosFolderProc.running) return
    runShort(photosFolderProc, ["photos-folder"])
  }
  function openVideosFolder() {
    if (!root.supported || videosFolderProc.running) return
    runShort(videosFolderProc, ["videos-folder"])
  }

  function setPack(path) {
    root.problem = ""
    saveSettings({ pack: String(path || "auto") })
    return root.pack
  }

  // A shell.json that can't be read or parsed (say, halfway through an edit) keeps the last good settings; until
  // the first good read nothing is taken over.
  function readConfig(text) {
    var config
    try { config = JSON.parse(text) } catch (e) { return }
    if (!config || typeof config !== "object" || Array.isArray(config)) return
    var seconds = Number(config.idle ? config.idle.screensaver : NaN)
    root.screensaverSeconds = isFinite(seconds) && seconds >= 0 ? Math.floor(seconds) : 150
    var lock = Number(config.idle ? config.idle.lock : NaN)
    root.lockSeconds = isFinite(lock) && lock >= 0 ? Math.floor(lock) : 300

    var found = null
    var layout = config.bar && config.bar.layout ? config.bar.layout : {}
    for (var section in layout) {
      var items = Array.isArray(layout[section]) ? layout[section] : []
      for (var i = 0; i < items.length && !found; i++)
        if (items[i] && items[i].id === root.pluginId) found = items[i]
    }
    var plugins = Array.isArray(config.plugins) ? config.plugins : []
    for (var j = 0; j < plugins.length && !found; j++)
      if (plugins[j] && plugins[j].id === root.pluginId) found = plugins[j]
    var next = {}
    if (found) for (var key in found) if (key !== "id") next[key] = found[key]
    root.entry = next

    if (!root.loaded) {
      root.loaded = true
      root.syncTakeover()
      root.refresh()
    }
  }

  onOnIdleChanged: if (root.loaded) root.syncTakeover()

  // ---- Omarchy's screensaver switch

  function syncTakeover() {
    if (!root.loaded) return
    if (!root.supported) {
      root.problem = "ScreenSafe doesn't run on " + (root.arch || "this computer")
      return
    }
    runShort(takeoverProc, root.onIdle ? ["takeover", "claim", root.token] : ["takeover", "release"])
  }

  function takeoverResult(out, code) {
    var line = lastLine(out)
    if (code !== 0) {
      root.holding = false
      root.problem = "Couldn't switch Omarchy's screensaver off"
      return
    }
    if (line === "owned" || line === "user-off") {
      root.holding = true
    } else if (line === "user-on") {
      // You switched Omarchy's screensaver back on yourself.
      root.holding = false
      root.problem = "Omarchy's screensaver was switched back on, so ScreenSafe stepped aside"
      if (root.onIdle) saveSettings({ onIdle: false })
    } else {
      root.holding = false
    }
  }

  // ---- packs

  // A thumbnail that finds its pack changed asks for the catalog again, at most once every 5 seconds, so a pack whose
  // files keep changing (one being written, say) can't keep the renderer running back to back. A change in between is
  // picked up when the 5 seconds are up.
  property real lastChangeRefresh: 0
  function refreshChanged() {
    var wait = root.lastChangeRefresh + 5000 - Date.now()
    if (wait <= 0) {
      root.lastChangeRefresh = Date.now()
      root.refresh()
    } else if (!changeRefresh.running) {
      changeRefresh.interval = Math.ceil(wait)
      changeRefresh.start()
    }
  }
  Timer {
    id: changeRefresh
    onTriggered: root.refreshChanged()
  }

  function refresh() {
    if (root.supported && !catalogProc.running)
      runShort(catalogProc, ["catalog", root.builtinPacks, root.userPacks, root.themeDir, "--theme", root.themeDir, "--branding", root.branding, "--logo", root.logo, "--badge", root.badge, "--photos", root.photos, "--videos", root.videos])
  }

  function catalogResult(out) {
    var list
    try { list = JSON.parse(String(out || "")) } catch (e) { return }
    if (!Array.isArray(list)) return
    if (list.length > 0) {
      var swatches = ColorModel.palette(list[0].palette)
      if (swatches.length > 0) root.themeSwatches = swatches
    }
    var packs = []
    for (var i = 0; i < list.length && packs.length < 64; i++) {
      var p = list[i]
      if (!p || typeof p.path !== "string") continue
      packs.push({
        path: p.path,
        ok: p.ok === true,
        name: String(p.name || "").slice(0, 60),
        author: String(p.author || "").slice(0, 60),
        description: String(p.description || "").slice(0, 300),
        error: String(p.error || "").slice(0, 300),
        settings: root.validDeclarations(p.settings),
        // How many photos it has to show, or -1 for a pack that doesn't show any.
        photos: typeof p.photos === "number" && isFinite(p.photos) && p.photos >= -1 ? Math.floor(Math.min(p.photos, 100000)) : -1,
        videos: typeof p.videos === "number" && isFinite(p.videos) && p.videos >= -1 ? Math.floor(Math.min(p.videos, 100000)) : -1,
        fingerprint: /^[0-9a-f]{16}$/.test(String(p.fingerprint || "")) ? String(p.fingerprint) : "",
        source: p.path === root.themePack ? "theme" : p.path.indexOf(root.builtinPacks + "/") === 0 ? "built-in" : "yours"
      })
    }
    root.packs = packs
    if (root.pack !== "auto" && !root.listed(root.pack))
      root.problem = "The chosen pack isn't available, so ScreenSafe shows Automatic's"
    else if (root.problem === "The chosen pack isn't available, so ScreenSafe shows Automatic's")
      root.problem = ""
    // Thumbnails are kept only for packs still listed; each is checked again when its row opens.
    var keep = {}
    for (var key in root.thumbnails) {
      var path = key.split("|")[0]
      for (var j = 0; j < packs.length; j++)
        if (packs[j].path === path) keep[key] = root.thumbnails[key]
    }
    root.thumbnails = keep
  }

  // ---- thumbnails: one frame of a pack, drawn small by the renderer and kept in memory (never on disk) with the
  // pack's fingerprint: a digest of its files, the theme's colors and backgrounds it uses, the art it shows, and the
  // renderer. When a card comes into view, its thumbnail is checked against the pack as it is now, which costs a
  // few milliseconds, and drawn again only if the pack has changed: someone working on a pack sees each change.

  // "path|size|settings" → { fingerprint, url }: url is "pixels W H <base64 RGB>", which the panel shows behind a
  // binary PPM header of its own, or "none" if it couldn't be drawn.
  property var thumbnails: ({})
  property var thumbQueue: []
  readonly property int thumbnailSide: 480

  function fingerprintOf(path) {
    for (var i = 0; i < root.packs.length; i++) if (root.packs[i].path === path) return root.packs[i].fingerprint
    return ""
  }

  function thumbnailPath(path) {
    path = String(path)
    return path === "auto" ? root.resolvedPack() : path
  }

  // The thumbnail for `path` at `size` ("352x220"): its pixels, "none" if it can't be drawn, or "" if it hasn't
  // been yet. A thumbnail being checked stays until it's replaced. Only looks; requestThumbnail() asks.
  function thumbnailOf(path, size) {
    path = root.thumbnailPath(path)
    if (root.fingerprintOf(path) === "") return "none"
    var entry = root.thumbnails[path + "|" + size + "|" + root.settingsArg(path)]
    return entry === undefined ? "" : entry.url
  }

  // Draws the thumbnail, or checks the one there is and draws it again if the pack has changed.
  function requestThumbnail(path, size) {
    path = root.thumbnailPath(path)
    var key = path + "|" + size + "|" + root.settingsArg(path)
    if (!root.supported || root.fingerprintOf(path) === "") return
    if (!/^[0-9]{2,3}x[0-9]{2,3}$/.test(String(size))) return
    // A drag can try many custom colors. Keep only the latest queued rendering per pack/size.
    root.thumbQueue = root.thumbQueue.filter(function(job) { return job.path !== path || job.size !== String(size) })
    if (thumbProc.running && thumbProc.key === key) return
    root.thumbQueue = root.thumbQueue.concat([{ key: key, path: path, size: String(size), settings: root.settingsArg(path) }])
    Qt.callLater(root.nextThumbnail)
  }

  function nextThumbnail() {
    if (thumbProc.running || root.thumbQueue.length === 0) return
    var job = root.thumbQueue[0]
    root.thumbQueue = root.thumbQueue.slice(1)
    var args = ["thumbnail", job.path, "--size", job.size, "--theme", root.themeDir, "--branding", root.branding, "--logo", root.logo, "--badge", root.badge, "--photos", root.photos, "--videos", root.videos]
    if (job.settings !== "") args = args.concat(["--settings", job.settings])
    var entry = root.thumbnails[job.key]
    if (entry !== undefined && entry.fingerprint !== "") args = args.concat(["--unless", entry.fingerprint])
    thumbProc.key = job.key
    thumbProc.path = job.path
    thumbProc.size = job.size
    runShort(thumbProc, args)
  }

  // "pixels W H <base64>": exactly the size asked for ("352x220"), at most thumbnailSide a side, and exactly three
  // bytes a pixel, so the image the panel shows is exactly what it expects.
  function validPixels(body, size) {
    var m = /^pixels ([0-9]{1,3}) ([0-9]{1,3}) ([A-Za-z0-9+\/]*)$/.exec(body)
    if (!m) return false
    var w = Number(m[1]), h = Number(m[2])
    return m[1] + "x" + m[2] === String(size) && w > 0 && h > 0 && w <= root.thumbnailSide && h <= root.thumbnailSide
      && m[3].length === w * h * 4
  }

  function thumbnailResult(key, path, size, out, code) {
    var lines = String(out || "").trim().split("\n")
    var fp = /^fingerprint ([0-9a-f]{16})$/.exec(lines[0] || "")
    var body = String(lines[1] || "")
    var next = Object.assign({}, root.thumbnails)
    if (code === 0 && fp && body === "unchanged" && next[key] !== undefined) {
      // As it was: nothing to do.
    } else if (code === 0 && fp && lines.length === 2 && root.validPixels(body, size)) {
      next[key] = { fingerprint: fp[1], url: body }
    } else {
      // It can't be drawn; it isn't asked for again until its row opens again.
      next[key] = { fingerprint: "", url: "none" }
    }
    // One thumbnail a pack and size, for the settings it was last drawn with, however many settings are tried.
    var prefix = path + "|" + size + "|"
    for (var k in next) if (k !== key && k.indexOf(prefix) === 0) delete next[k]
    root.thumbnails = next
    // The pack has changed since it was listed, or can't be used now: list it again, so its name, description or
    // error is as it is now too.
    if (!fp || fp[1] !== root.fingerprintOf(path)) root.refreshChanged()
    Qt.callLater(root.nextThumbnail)
  }

  // ---- showing it

  property bool fromIdle: false
  property string launching: ""
  // Set when the screensaver is closed because the session locked, rather than dismissed.
  property bool stoppedForLock: false
  // A preview has ended: `dismissed` when someone closed it themselves, not when the session locked or it failed.
  signal previewEnded(bool dismissed)

  // Open the screensaver: from idle (checking stay-awake, the switch and the lock first) or as a preview.
  function start(idle) {
    if (!root.supported) return "unsupported"
    if (rendererProc.running || statusProc.running || lockProc.running) return "already starting"
    root.fromIdle = !!idle
    runShort(statusProc, ["takeover", "status"])
    return "starting"
  }

  // A preview of the chosen pack, or of `path`, if it's one the catalog lists (from a pack's settings page).
  property string previewPath: ""
  function preview(path) {
    root.previewPath = typeof path === "string" && path !== "" && root.listed(path) ? path : ""
    return start(false)
  }

  function statusResult(out) {
    var status
    try { status = JSON.parse(String(out || "")) } catch (e) { status = null }
    if (!status) return
    if (root.fromIdle) {
      if (!root.onIdle || status.stayAwake === true) return
      if (status.omarchyOff !== true) {
        // Omarchy's screensaver was switched back on since ScreenSafe took over.
        root.takeoverResult("user-on", 0)
        runShort(takeoverProc, ["takeover", "release"])
        return
      }
    }
    lockProc.command = [root.omarchyPath + "/bin/omarchy-shell", "lock", "isLocked"]
    lockProc.environment = root.environment(true)
    lockProc.purpose = "start"
    root.begin(lockProc)
  }

  function lockResult(out, purpose) {
    var locked = lastLine(out) === "true"
    if (purpose === "watch") {
      if (locked) {
        root.stoppedForLock = true
        rendererProc.running = false
      }
      return
    }
    // Shown only when Omarchy plainly says the session isn't locked; any other answer, or none, keeps it hidden.
    if (lastLine(out) === "false") launch(!root.fromIdle && root.previewPath !== "" ? root.previewPath : root.resolvedPack())
  }

  function launch(packPath) {
    root.launching = packPath
    root.stoppedForLock = false
    rendererProc.command = [root.renderer, "run", "--pack", packPath, "--theme", root.themeDir, "--branding", root.branding, "--logo", root.logo, "--badge", root.badge, "--photos", root.photos, "--videos", root.videos, "--settings", root.settingsArg(packPath), "--role", "org.omarchy.screensaver", "--overlay", root.overlayArgument]
    if (root.performanceValues.enabled === 1) rendererProc.command = root.cacheCommand("run", packPath)
    rendererProc.environment = root.performanceValues.enabled === 1 ? root.cacheEnvironment() : root.environment(false)
    rendererOut.reset()
    rendererErr.reset()
    rendererProc.running = true
  }

  function rendererExited(code, errors) {
    if (code === 3 && root.launching !== root.defaultPack) {
      // The pack can't be used: say so, and show the built-in one instead.
      root.problem = root.nameOf(root.launching) + " can't be used: " + lastLine(errors).replace(/^screensafe: /, "")
      root.launch(root.defaultPack)
      return
    } else if (code !== 0 && code !== 143 && code !== 15) {
      root.problem = "Couldn't show the screensaver: " + lastLine(errors).replace(/^screensafe: /, "")
    } else if (root.launching === root.resolvedPack()) {
      root.problem = ""
    }
    if (!root.fromIdle) root.previewEnded(code === 0 && !root.stoppedForLock)
  }

  function statusJson() {
    return JSON.stringify({
      onIdle: root.onIdle,
      pack: root.pack,
      resolvedPack: root.resolvedPack(),
      holdingOmarchyOff: root.holding,
      running: root.running,
      idleSeconds: root.idleSeconds,
      ownIdleSeconds: root.ownSeconds,
      favorites: root.favorites,
      omarchyIdleSeconds: root.screensaverSeconds,
      omarchyLockSeconds: root.lockSeconds,
      packs: root.packs.length,
      arch: root.arch,
      problem: root.problem
    })
  }

  // Short calls: one at a time each, each stopped if it runs for more than ten seconds.
  readonly property int deadlineMs: 10000
  readonly property var shortProcs: [takeoverProc, catalogProc, statusProc, lockProc, thumbProc, photosFolderProc, videosFolderProc, systemProc, cacheStatusProc]

  function runShort(proc, args) {
    if (proc.running) return
    proc.command = [root.renderer].concat(args)
    proc.environment = root.environment(false)
    begin(proc)
  }

  function begin(proc) {
    proc.startedAt = Date.now()
    if (proc.stdout && typeof proc.stdout.reset === "function") proc.stdout.reset()
    if (proc.stderr && typeof proc.stderr.reset === "function") proc.stderr.reset()
    proc.running = true
  }

  Timer {
    id: deadline
    interval: 1000
    repeat: true
    running: root.shortProcs.some(function(proc) { return proc.running })
    onTriggered: {
      var now = Date.now()
      for (var i = 0; i < root.shortProcs.length; i++) {
        var p = root.shortProcs[i]
        if (p.running && now - p.startedAt > root.deadlineMs) p.running = false
      }
    }
  }

  // A process's output, read as it arrives and limited in size: past `limit` characters the process is stopped and
  // what it said is dropped (`over`), rather than held in the shell. Each limit leaves room over the most the renderer
  // or Omarchy can legitimately print.
  component BoundedOutput: SplitParser {
    property int limit: 4096
    property var process: null
    property string text: ""
    property bool over: false
    function reset() { text = ""; over = false }
    splitMarker: ""
    onRead: data => {
      if (over) return
      if (text.length + data.length > limit) {
        over = true
        text = ""
        if (process) process.running = false
        return
      }
      text += data
    }
  }

  Process {
    id: takeoverProc
    property real startedAt: 0
    clearEnvironment: true
    stdout: BoundedOutput { id: takeoverOut; limit: 16384; process: takeoverProc }
    onExited: function(code) { root.takeoverResult(takeoverOut.text, code) }
  }

  Process {
    id: catalogProc
    property real startedAt: 0
    clearEnvironment: true
    // The renderer stops a catalog at 64 KiB.
    stdout: BoundedOutput { id: catalogOut; limit: 131072; process: catalogProc }
    onExited: function(code) { if (code === 0 && !catalogOut.over) root.catalogResult(catalogOut.text) }
  }

  Process {
    id: systemProc
    property real startedAt: 0
    clearEnvironment: true
    stdout: BoundedOutput { id: systemOut; limit: 512; process: systemProc }
    stderr: BoundedOutput { id: systemErr; limit: 1024; process: systemProc }
    onExited: function(code) {
      var data = null
      if (code === 0 && !systemOut.over) { try { data = JSON.parse(systemOut.text) } catch (e) {} }
      root.systemReading = OverlayModel.reading(data, root.systemReading)
      root.pushMetrics()
    }
  }

  Process {
    id: thumbProc
    property real startedAt: 0
    property string key: ""
    property string path: ""
    property string size: ""
    clearEnvironment: true
    // At most 480 by 480 pixels, four characters each, and a short line before: under 922 KB.
    stdout: BoundedOutput { id: thumbOut; limit: 1048576; process: thumbProc }
    onExited: function(code) { root.thumbnailResult(thumbProc.key, thumbProc.path, thumbProc.size, thumbOut.over ? "" : thumbOut.text, code) }
  }

  Process {
    id: statusProc
    property real startedAt: 0
    clearEnvironment: true
    stdout: BoundedOutput { id: statusOut; limit: 16384; process: statusProc }
    onExited: function(code) { if (code === 0 && !statusOut.over) root.statusResult(statusOut.text) }
  }

  Process {
    id: lockProc
    property string purpose: "start"
    property real startedAt: 0
    clearEnvironment: true
    stdout: BoundedOutput { id: lockOut; limit: 4096; process: lockProc }
    onExited: function() { root.lockResult(lockOut.over ? "" : lockOut.text, lockProc.purpose) }
  }

  Process {
    id: photosFolderProc
    property real startedAt: 0
    clearEnvironment: true
    stdout: BoundedOutput { id: photosFolderOut; limit: 4096; process: photosFolderProc }
    onExited: function(code) {
      if (code === 0 && !photosFolderOut.over && lastLine(photosFolderOut.text) === root.photos)
        Qt.openUrlExternally("file://" + encodeURI(root.photos))
    }
  }

  Process {
    id: videosFolderProc
    property real startedAt: 0
    clearEnvironment: true
    stdout: BoundedOutput { id: videosFolderOut; limit: 4096; process: videosFolderProc }
    onExited: function(code) {
      if (code === 0 && !videosFolderOut.over && lastLine(videosFolderOut.text) === root.videos)
        Qt.openUrlExternally("file://" + encodeURI(root.videos))
    }
  }

  Process {
    id: livePreviewProc
    clearEnvironment: true
    stdinEnabled: true
    stdout: BoundedOutput { id: liveOut; limit: 4096 }
    stderr: BoundedOutput { id: liveErr; limit: 16384 }
    // The one that was running has gone: start the one waiting, if any, now that it's safe to.
    onExited: function(code) {
      if (!root.liveStopping && code !== 0 && root.livePath !== "") root.liveFailed = root.livePath
      root.liveStopping = false
      var next = root.livePending
      root.livePending = null
      if (next) root.launchLive(next.path, next.args, next.region)
      else { root.livePath = ""; root.liveRegion = "" }
    }
  }

  Process {
    id: rendererProc
    stdinEnabled: true
    clearEnvironment: true
    stdout: BoundedOutput { id: rendererOut; limit: 4096 }
    // Only its last line is shown, when it fails; a renderer that says more is cut off, but keeps running.
    stderr: BoundedOutput { id: rendererErr; limit: 65536 }
    onExited: function(code) { root.rendererExited(code, rendererErr.text) }
  }

  Process {
    id: cacheStatusProc
    property real startedAt: 0
    clearEnvironment: true
    stdout: BoundedOutput { id: cacheStatusOut; limit: 2048; process: cacheStatusProc }
    stderr: BoundedOutput { id: cacheStatusErr; limit: 2048; process: cacheStatusProc }
    onExited: function(code) {
      var data = null
      if (code === 0 && !cacheStatusOut.over) { try { data = JSON.parse(cacheStatusOut.text) } catch (e) {} }
      root.performanceState = data && ["ready", "empty", "ineligible", "unavailable"].indexOf(data.state) >= 0 ? data.state : "unavailable"
    }
  }
  Process {
    id: cacheWorkProc
    property real startedAt: 0
    property string action: ""
    property bool cancelled: false
    clearEnvironment: true
    stdout: BoundedOutput { id: cacheWorkOut; limit: 2048; process: cacheWorkProc }
    stderr: BoundedOutput { id: cacheWorkErr; limit: 8192; process: cacheWorkProc }
    onExited: function(code) {
      var data = null
      if (!cacheWorkOut.over) { try { data = JSON.parse(cacheWorkOut.text) } catch (e) {} }
      root.performanceState = cacheWorkProc.cancelled ? "cancelled" : data && ["ready", "empty", "ineligible", "cancelled", "unavailable"].indexOf(data.state) >= 0 ? data.state : "unavailable"
      cacheWorkProc.cancelled = false
      if (root.pendingCacheAction !== "") { var pending = root.pendingCacheAction; root.pendingCacheAction = ""; Qt.callLater(function() { root.cacheMaintenance(pending) }) }
      else if (cacheWorkProc.action === "trim") Qt.callLater(root.refreshPerformance)
    }
  }
  Timer {
    interval: 1000
    repeat: true
    running: cacheWorkProc.running
    onTriggered: if (Date.now() - cacheWorkProc.startedAt > 1200000) { cacheWorkProc.cancelled = true; cacheWorkProc.running = false }
  }

  // While the screensaver is up, close it if the session locks some other way than Omarchy's own lock command.
  Timer {
    interval: 5000
    repeat: true
    running: rendererProc.running
    onTriggered: {
      if (lockProc.running) return
      lockProc.command = [root.omarchyPath + "/bin/omarchy-shell", "lock", "isLocked"]
      lockProc.environment = root.environment(true)
      lockProc.purpose = "watch"
      root.begin(lockProc)
    }
  }

  // Quickshell 0.3.1's IdleMonitor goes silent when its timeout changes while it runs (Omarchy issue 14438), so it
  // stays off until the settings have loaded, and goes off and on again whenever the timeout changes.
  property bool reviving: false
  readonly property int idleTimeout: root.ownSeconds > 0 ? root.ownSeconds : root.screensaverSeconds + 1
  onIdleTimeoutChanged: {
    root.reviving = true
    Qt.callLater(function() { root.reviving = false })
  }

  // After ScreenSafe's own idle time; or, following Omarchy's, one second after Omarchy's own screensaver would
  // have opened, so its idle cycle has begun.
  IdleMonitor {
    enabled: root.loaded && root.onIdle && root.idleSeconds > 0 && !root.reviving
    timeout: root.idleTimeout
    respectInhibitors: true
    onIsIdleChanged: if (isIdle) root.start(true)
  }

  FileView {
    id: archFile
    path: "/proc/sys/kernel/arch"
    blockLoading: true
    printErrors: false
  }

  FileView {
    path: root.home + "/.config/omarchy/shell.json"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.readConfig(text())
  }

  // A new theme may bring its own pack.
  FileView {
    path: root.home + "/.local/state/omarchy/current/theme.name"
    watchChanges: true
    printErrors: false
    onFileChanged: { reload(); root.refresh() }
  }

  // Unloading the plugin (disabling or removing it, or the shell exiting) closes the screensaver and gives Omarchy
  // its own back.
  Component.onDestruction: {
    cacheWorkProc.running = false
    cacheStatusProc.running = false
    for (var i = 0; i < root.shortProcs.length; i++) root.shortProcs[i].running = false
    if (rendererProc.running) rendererProc.write("quit\n")
    if (livePreviewProc.running) livePreviewProc.write("quit\n")
    rendererProc.running = false
    livePreviewProc.running = false
    if (root.supported) Quickshell.execDetached({
      command: [root.renderer, "takeover", "release", root.token],
      environment: root.environment(false),
      clearEnvironment: true
    })
  }

  IpcHandler {
    target: "screensafe"

    function preview(): string { return root.preview("") }
    // A listed pack's settings: what it offers, and the values chosen, as JSON.
    function settings(path: string): string {
      if (!root.listed(path)) return "unknown pack"
      return JSON.stringify({ offered: root.declarationsOf(path), chosen: root.settingsOf(path) })
    }
    // Sets one of a listed pack's settings; returns the value kept, which is always one the pack allows.
    function setting(path: string, name: string, value: string): string {
      return root.setSetting(path, name, value === "true" ? 1 : value === "false" ? 0 : Number(value))
    }
    function enable(): string { return root.setOnIdle(true) }
    function disable(): string { return root.setOnIdle(false) }
    function refresh(): string { root.refresh(); return "ok" }
    // "auto", or the path of a pack the panel lists.
    function pack(path: string): string {
      if (path === "auto") return root.setPack("auto")
      for (var i = 0; i < root.packs.length; i++)
        if (root.packs[i].path === path && root.packs[i].ok) return root.setPack(path)
      return "unknown pack"
    }
    // Stars or unstars a pack the panel lists, by its path.
    function favorite(path: string): string {
      for (var i = 0; i < root.packs.length; i++)
        if (root.packs[i].path === path) return root.toggleFavorite(path)
      return "unknown pack"
    }
    // Seconds idle before ScreenSafe shows, from 10 to 86400; or 0 or "omarchy" to follow Omarchy's.
    function delay(seconds: string): string { return root.setIdleSeconds(seconds) }
    function overlaySettings(): string { return JSON.stringify({ offered: root.overlayDeclarations, chosen: root.overlayValues }) }
    function overlaySetting(name: string, value: string): string { return root.setOverlay(name, Number(value)) }
    function status(): string { return root.statusJson() }
  }
}
