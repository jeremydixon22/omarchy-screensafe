import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "OverlayModel.js" as OverlayModel
import "PerformanceModel.js" as PerformanceModel
import "ColorModel.js" as ColorModel

// The bar icon and its panel: whether ScreenSafe is the screensaver, which pack it shows, and a preview. The
// service does the work; this is a handle on it. Every string from a pack is shown as plain text. The
// thumbnail and description stay visible in each card; the chosen pack is ticked. Starred
// packs are listed first, by name; the title stays pinned above the list, and Preview below it.
Panel {
  id: root
  moduleName: "io.github.jeremydixon22.screensafe"
  ipcTarget: "io.github.jeremydixon22.screensafe"

  readonly property var svc: bar && bar.shell ? bar.shell.serviceFor("io.github.jeremydixon22.screensafe") : null
  readonly property bool onIdle: svc ? svc.onIdle === true : false
  readonly property string pack: svc ? String(svc.pack) : "auto"
  readonly property var packs: svc && Array.isArray(svc.packs) ? svc.packs : []
  readonly property var favorites: svc && Array.isArray(svc.favorites) ? svc.favorites : []
  function isFavorite(path) { return root.favorites.indexOf(path) >= 0 }
  // The starred packs, by name, then the rest in the order they're listed.
  readonly property var favoritePacks: root.packs.filter(function(p) { return root.isFavorite(p.path) })
    .sort(function(a, b) { return String(a.name).toLowerCase().localeCompare(String(b.name).toLowerCase()) })
  readonly property var otherPacks: root.packs.filter(function(p) { return !root.isFavorite(p.path) })
  readonly property int idleSeconds: svc ? svc.idleSeconds : 150
  readonly property int ownSeconds: svc ? svc.ownSeconds : 0
  readonly property int omarchySeconds: svc ? svc.screensaverSeconds : 150
  readonly property int lockSeconds: svc ? svc.lockSeconds : 300
  readonly property bool locksFirst: lockSeconds > 0 && idleSeconds >= lockSeconds
  readonly property string problem: svc && svc.problem ? String(svc.problem) : ""

  readonly property color foreground: bar ? bar.foreground : Commons.Color.foreground
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family

  // Screensaver, Settings and About tabs. The panel reopens on the one it was left on, so someone
  // following About's links comes back to them.
  property string tab: "screensaver"
  property string settingsTab: "overlay"
  readonly property bool overlayPageActive: root.tab === "settings" && root.settingsTab === "overlay"
  readonly property bool performancePageActive: root.tab === "settings" && root.settingsTab === "performance"
  function setSettingsTab(value) {
    root.settingsTab = value
    if (value === "performance" && root.svc) root.svc.refreshPerformance()
    overlayScroller.contentY = 0
    performanceScroller.contentY = 0
  }
  function setTab(t) {
    if (t === root.tab) return
    root.settingsPath = ""
    root.tab = t
    if (t === "settings" && root.settingsTab === "performance" && root.svc) root.svc.refreshPerformance()
    root.setScroll(0)
  }

  // About's links: the project's own pages and its catalog of shared packs, and nothing else.
  readonly property string repoUrl: "https://github.com/jeremydixon22/omarchy-screensafe"
  // The catalog is a repository of its own, so that this one holds only the plugin as submitted for review.
  readonly property string catalogUrl: "https://github.com/jeremydixon22/omarchy-screensafe-packs"
  readonly property var links: ({
    "about:project": root.repoUrl,
    "about:find": root.catalogUrl + "#readme",
    "about:make": root.repoUrl + "/blob/main/docs/MAKING.md",
    "about:share": root.repoUrl + "/blob/main/docs/COMMUNITY.md"
  })
  // Within one of the two repositories exactly: the repository's page, or a page beneath it.
  function ownPage(url) {
    var bases = [root.repoUrl, root.catalogUrl]
    for (var i = 0; i < bases.length; i++) {
      var rest = url.slice(bases[i].length)
      if (url.indexOf(bases[i]) === 0 && (rest === "" || rest[0] === "/" || rest[0] === "#")) return true
    }
    return false
  }
  function openLink(row) {
    var url = root.links[row]
    if (typeof url !== "string" || !root.ownPage(url)) return
    root.close()
    Qt.openUrlExternally(url)
  }

  // The keyboard cursor walks these rows top to bottom.
  readonly property var rows: {
    // In the order they appear: the time at the top, then the tabs and Enable, then the list.
    if (root.overlayPageActive) {
      var overlayRows = ["delay", "tabs", "enable", "settings:tabs"]
      var declared = root.overlayScrollingDecl
      for (var o = 0; o < declared.length; o++) overlayRows.push("set:" + declared[o].name)
      return overlayRows.concat(["overlay:reset"]).concat(root.overlayPositionDecl ? ["set:position", "preview"] : ["preview"])
    }
    if (root.performancePageActive) return ["delay", "tabs", "enable", "settings:tabs"].concat(PerformanceModel.declarations.map(function(d) { return "set:" + d.name })).concat(["performance:prepare", "performance:clear"])
    if (root.tab === "about") return ["delay", "tabs", "enable", "about:project", "about:find", "about:make", "about:share"]
    if (root.settingsPath !== "") {
      var page = ["delay", "tabs", "enable", "settings:back", "settings:live"]
      if (root.settingsPhotos >= 0) page.push("settings:photos")
      if (root.settingsVideos >= 0) page.push("settings:videos")
      for (var s = 0; s < root.scrollingSettingsDecl.length; s++) {
        var declaration = root.scrollingSettingsDecl[s]
        if (!root.settingActive(declaration)) continue
        page.push("set:" + declaration.name)
        if (declaration.kind === "color" && root.settingsValues[declaration.name] >= ColorModel.customBase)
          page = page.concat(["color:" + declaration.name + ":h", "color:" + declaration.name + ":s", "color:" + declaration.name + ":v"])
      }
      page.push("settings:reset")
      if (root.pinnedColorDecl) {
        page.push("set:" + root.pinnedColorDecl.name)
        if (root.settingsValues[root.pinnedColorDecl.name] >= ColorModel.customBase)
          page = page.concat(["color:" + root.pinnedColorDecl.name + ":h", "color:" + root.pinnedColorDecl.name + ":s", "color:" + root.pinnedColorDecl.name + ":v"])
      }
      // With a live preview on the page, there's no need for the pinned one.
      if (!root.liveOn) page.push("preview")
      return page
    }
    var list = ["delay", "tabs", "enable", "pack:auto"]
    var ordered = root.favoritePacks.concat(root.otherPacks)
    for (var i = 0; i < ordered.length; i++) list.push("pack:" + ordered[i].path)
    list.push("preview")
    return list
  }
  property int cursor: 0
  property bool cursorActive: false
  // The row the cursor is on, so it stays on that row when the list is reordered.
  property string cursorRow: ""
  // The row the cursor was last on, kept while the panel is closed: opening it again starts there, or, if that row
  // has gone, on the chosen pack, or else at the top.
  property string lastRow: ""
  onCursorChanged: {
    cursorRow = rows[cursor] || ""
    if (cursorActive) lastRow = cursorRow
  }
  onRowsChanged: {
    if (cursorRow !== "" && rows.indexOf(cursorRow) >= 0) cursor = rows.indexOf(cursorRow)
    else cursor = Math.max(0, Math.min(cursor, rows.length - 1))
  }
  function at(row) { return root.cursorActive && root.rows[root.cursor] === row }
  // Whether the cursor last moved by keyboard. Only then does the list scroll to follow it: a row the pointer is on
  // is already in view, and scrolling under the pointer would hide that the list goes on.
  property bool keyMoved: false
  // The tabs and the switch report hovering on their own, sometimes as the panel opens under a resting pointer. That
  // moves the cursor only once the pointer has moved in the panel.
  // Not while the keyboard has the cursor: rows scrolled under a resting pointer report hovering too. It's the
  // keyboard's until the pointer itself moves (see pointFrom).
  function hoverPoint(row) { if (root.pointerSeen && !root.keyMoved) root.point(row) }
  function point(row) {
    root.keyMoved = false
    root.showChosen = false
    root.cursorActive = true
    root.cursor = Math.max(0, root.rows.indexOf(row))
  }
  // When the panel opens, the list is scrolled to show the cursor's row (or else the chosen pack), and kept there
  // as its rows settle, until the cursor first moves.
  property bool showChosen: false
  property string openRow: ""
  function scrollToChosen() {
    if (!root.showChosen || root.tab !== "screensaver") return
    var kids = packList.children
    var byRow = false
    for (var j = 0; j < kids.length; j++) if (kids[j].visible && kids[j].row === root.openRow) byRow = true
    for (var i = 0; i < kids.length; i++) {
      if (kids[i].visible && (byRow ? kids[i].row === root.openRow : kids[i].chosen === true)) {
        root.setScroll(0)
        root.scrollTo(kids[i])
        return
      }
    }
  }
  // The settings page's live preview: the renderer drawing the pack, laid over the thumbnail's box while the page is
  // open; its fixed box stays above the scrolling options. The thumbnail remains beneath it until it shows.
  readonly property bool liveOn: svc ? svc.livePreviews : true
  // Started once a page has stayed open a moment, so passing through pages quickly starts nothing.
  property bool liveSettled: false
  Timer {
    id: liveSettle
    interval: 350
    onTriggered: root.liveSettled = true
  }
  readonly property bool liveWanted: root.opened && root.tab === "screensaver" && root.settingsPath !== "" && root.liveOn
    && root.liveSettled
  onLiveWantedChanged: root.syncLive()
  function syncLive() {
    if (!root.svc) return
    if (!root.liveWanted || !pageThumbBox.visible || pageThumbBox.width < 18 || pageThumbBox.height < 11) { root.svc.stopLive(); return }
    var top = pageThumbBox.mapToItem(settingsView, 0, 0).y
    if (top < 0 || top + pageThumbBox.height > settingsView.height) { root.svc.stopLive(); return }
    // The panel covers its whole screen, so a place in it is a place on that screen, in logical pixels.
    var at = pageThumbBox.mapToItem(null, 1, 1)
    var screen = panel.screen ? panel.screen.name : ""
    root.svc.startLive(root.settingsPath, at.x, at.y, pageThumbBox.width - 2, pageThumbBox.height - 2, screen)
  }
  Timer {
    interval: 300
    repeat: true
    running: root.liveWanted
    onTriggered: root.syncLive()
  }

  // The settings page's thumbnail: drawn again whenever the settings change, since its key includes them.
  readonly property string pageThumb: settingsPath !== "" && svc ? svc.thumbnailOf(settingsPath, root.thumbSize) : ""
  onPageThumbChanged: if (settingsPath !== "" && pageThumb === "" && svc) svc.requestThumbnail(settingsPath, root.thumbSize)
  onSettingsPathChanged: {
    root.liveSettled = false
    if (settingsPath !== "") liveSettle.restart()
    if (!svc) return
    if (settingsPath !== "") svc.requestThumbnail(settingsPath, root.thumbSize)
    else svc.stopLive()
  }
  function inside(item, ancestor) {
    for (var node = item; node; node = node.parent) if (node === ancestor) return true
    return false
  }
  // Scroll only the control's own region; pinned rows never move the options.
  function revealInPage(item) {
    if ((!root.overlayPageActive && item === backRow) || root.inside(item, overlayPositionDock)) return
    var page = root.overlayPageActive ? overlayPage : root.performancePageActive ? performancePage : settingsPage
    var scroller = root.overlayPageActive ? overlayScroller : root.performancePageActive ? performanceScroller : pageScroller
    if (root.inside(item, colorDockPage)) { page = colorDockPage; scroller = colorDock }
    var top = item.mapToItem(page, 0, 0).y
    var bottom = top + item.height
    var view = scroller.height
    if (bottom > scroller.contentY + view) scroller.contentY = Math.min(Math.max(0, page.implicitHeight - view), bottom - view)
    if (top < scroller.contentY) scroller.contentY = Math.max(0, top)
  }

  onCursorActiveChanged: if (cursorActive) lastRow = rows[cursor] || ""
  // Thumbnails are drawn at the screen's shape, at about twice the size they're shown.
  readonly property real screenAspect: panel.screenW > 0 && panel.screenH > 0
    ? Math.max(1.0, Math.min(2.4, panel.screenW / panel.screenH)) : 16 / 10
  readonly property string thumbSize: "352x" + Math.round(352 / root.screenAspect)
  // Pointing with the mouse moves the cursor only when the pointer itself moves, so neither the panel opening under
  // a resting pointer nor rows scrolling under it take the cursor from the keyboard. The first event after opening
  // only notes where the pointer is.
  property point lastPointer: Qt.point(-1, -1)
  property bool pointerSeen: false
  function pointFrom(area, mouse, row) {
    var at = area.mapToGlobal(mouse.x, mouse.y)
    if (!root.pointerSeen) { root.pointerSeen = true; root.lastPointer = at; return }
    if (Math.abs(at.x - root.lastPointer.x) < 1 && Math.abs(at.y - root.lastPointer.y) < 1) return
    root.lastPointer = at
    root.point(row)
  }
  // Keyboard navigation reveals a whole card. Pointer movement never scrolls the list.
  property Item revealTarget: null
  function reveal(item) {
    root.revealTarget = item
    if (root.keyMoved) scrollTo(item)
  }
  function setScroll(y) { scroller.contentY = y }
  function scrollTo(item) {
    var peek = Style.space(16)
    var top = item.mapToItem(column, 0, 0).y
    var bottom = top + item.height
    var to = scroller.contentY
    if (bottom > to + scroller.height) to = bottom + peek - scroller.height
    if (top < to) to = top - peek
    root.setScroll(Math.max(0, Math.min(Math.max(0, scroller.contentHeight - scroller.height), to)))
  }
  // Keep list and settings views equally tall; expanded cards no longer need row-cut sizing.
  function panelHeight() { return panel.fittedContentHeight(Style.space(620), Style.space(620)) }

  function durationText(s) {
    if (s < 60) return s + " s"
    if (s >= 3600 && s % 3600 === 0) return (s / 3600) + " h"
    return Math.floor(s / 60) + " min" + (s % 60 ? " " + (s % 60) + " s" : "")
  }

  // Under the title: on or off. Anything in the way shows in a row pinned to the panel's bottom.
  readonly property string statusText: onIdle ? "On" : "Off"
  readonly property string alertText: problem !== ""
    ? problem
    : onIdle && locksFirst
      ? "Omarchy locks first, at " + durationText(lockSeconds) + ", so ScreenSafe won't show. Choose a shorter time."
      : ""

  readonly property string delayText: "After " + durationText(idleSeconds) + " idle"
  readonly property string delayDetail: (ownSeconds > 0 ? "ScreenSafe's own time" : "Omarchy's idle time")
    + (locksFirst ? " · but Omarchy locks first, at " : " · Omarchy locks at ") + durationText(lockSeconds)

  function stepDelay(direction) { if (svc) svc.stepIdleSeconds(direction) }

  function sourceText(source) {
    return source === "theme" ? "From the theme" : source === "built-in" ? "Built in" : "Your pack"
  }

  function packDetail(p) {
    if (p.ok !== true) return "Can't be used"
    return root.sourceText(p.source) + (p.author ? " · " + p.author : "")
  }

  function packInfo(p) {
    if (p.ok !== true) return "Can't be used: " + p.error
    return p.description !== "" ? p.description : "No description"
  }

  function toggleOnIdle() { if (svc) svc.setOnIdle(!root.onIdle) }

  // Stars or unstars a pack; the cursor stays on it as it moves in the list.
  function toggleFavorite(row) {
    if (!svc || row.indexOf("pack:") !== 0 || row === "pack:auto") return
    svc.toggleFavorite(row.slice(5))
  }

  function choose(row) {
    if (!svc) return
    var path = row.slice(5)
    if (path === "auto") { svc.setPack("auto"); return }
    for (var i = 0; i < root.packs.length; i++)
      if (root.packs[i].path === path && root.packs[i].ok) svc.setPack(path)
  }

  // ---- a pack's settings page, shown in place of the list: the pack whose settings it shows, or "".
  property string settingsPath: ""
  // The row to put the cursor back on when the page closes.
  property string settingsReturn: ""
  // Kept open across a preview started from it, so the panel comes back to the page.
  property bool keepSettingsPage: false
  readonly property var settingsDecl: settingsPath !== "" && svc ? svc.declarationsOf(settingsPath) : []
  readonly property var settingsValues: settingsPath !== "" && svc ? svc.settingsOf(settingsPath) : ({})
  readonly property var pinnedColorDecl: {
    for (var i = 0; i < root.settingsDecl.length; i++)
      if (root.settingsDecl[i].kind === "color" && root.settingActive(root.settingsDecl[i])) return root.settingsDecl[i]
    return null
  }
  readonly property var scrollingSettingsDecl: root.settingsDecl.filter(function(d) { return !root.pinnedColorDecl || d.name !== root.pinnedColorDecl.name })
  readonly property var overlayPositionDecl: {
    var declared = root.svc ? root.svc.overlayDeclarations : []
    for (var i = 0; i < declared.length; i++) if (declared[i].name === "position") return declared[i]
    return null
  }
  readonly property var overlayScrollingDecl: (root.svc ? root.svc.overlayDeclarations : []).filter(function(d) { return d.name !== "position" })
  readonly property string settingsName: {
    for (var i = 0; i < root.packs.length; i++) if (root.packs[i].path === root.settingsPath) return root.packs[i].name
    return ""
  }
  function hasSettings(path) { return !!root.svc && root.svc.declarationsOf(path).length > 0 }
  // How many photos the page's pack has to show, or -1 for a pack that doesn't show photos.
  readonly property int settingsPhotos: settingsPath !== "" && svc ? svc.photosOf(settingsPath) : -1
  function openPhotos() {
    if (!root.svc) return
    root.close()
    root.svc.openPhotosFolder()
  }
  readonly property int settingsVideos: settingsPath !== "" && svc ? svc.videosOf(settingsPath) : -1
  function openVideos() {
    if (!root.svc) return
    root.close()
    root.svc.openVideosFolder()
  }

  function openSettings(row) {
    if (row.indexOf("pack:") !== 0 || row === "pack:auto" || !root.hasSettings(row.slice(5))) return
    root.settingsReturn = row
    root.settingsPath = row.slice(5)
    root.cursorActive = true
    var first = root.settingsDecl.length > 0 ? "set:" + root.settingsDecl[0].name : "settings:back"
    root.cursor = Math.max(0, root.rows.indexOf(first))
    pageScroller.contentY = 0
  }

  function closeSettings() {
    var back = root.settingsReturn
    root.settingsPath = ""
    var i = root.rows.indexOf(back)
    if (i >= 0) root.cursor = i
  }

  function declOf(name) {
    var list = root.performancePageActive ? PerformanceModel.declarations : root.overlayPageActive && root.svc ? root.svc.overlayDeclarations : root.settingsDecl
    for (var i = 0; i < list.length; i++) if (list[i].name === name) return list[i]
    return null
  }
  function settingActive(d) { return !d.when || root.settingsValues[d.when] === d.whenValue }

  // Changes a setting by a step: a toggle flips; a choice moves to the next or previous one (going round when
  // `wrap`); a range moves by its step, within its bounds.
  function stepSetting(name, direction, wrap) {
    var d = root.declOf(name)
    if (!d || !root.svc) return
    var v = (root.performancePageActive ? root.svc.performanceValues : root.overlayPageActive ? root.svc.overlayValues : root.settingsValues)[name]
    var next = v
    if (d.kind === "toggle") next = v ? 0 : 1
    else if (d.kind === "color") next = ColorModel.step(v, direction, wrap, root.svc.themeSwatches)
    else if (d.kind === "choice") {
      var i = Math.max(0, d.values.indexOf(v)) + direction
      next = d.values[wrap ? (i + d.values.length) % d.values.length : Math.max(0, Math.min(d.values.length - 1, i))]
    } else next = Math.max(d.min, Math.min(d.max, v + direction * d.step))
    if (next !== v) root.setSettingValue(name, next)
  }

  function setSettingValue(name, value) {
    if (!root.svc) return
    if (root.performancePageActive) root.svc.setPerformance(name, value)
    else if (root.overlayPageActive) root.svc.setOverlay(name, value)
    else root.svc.setSetting(root.settingsPath, name, value)
  }

  function settingValueText(d) {
    var v = (root.performancePageActive && root.svc ? root.svc.performanceValues : root.overlayPageActive && root.svc ? root.svc.overlayValues : root.settingsValues)[d.name]
    if (root.overlayPageActive && d.name === "opacity") return Math.round(v * 100) + "%"
    if (d.kind === "range") return (Math.round(v * 1000) / 1000) + (d.unit !== "" ? " " + d.unit : "")
    return ""
  }

  // Close the panel first, so the screensaver gets the keyboard.
  // A preview started here brings the panel back when it's dismissed, so trying one pack after another is quick. It
  // doesn't when the preview fails, when the session locks during it, or for a preview started any other way.
  property bool reopenAfterPreview: false
  function preview() {
    root.reopenAfterPreview = true
    root.previewPath = root.settingsPath
    root.close()
    startTimer.restart()
  }
  // From a settings page, the preview shows that pack, as its settings now are.
  property string previewPath: ""
  Connections {
    target: root.svc
    function onPreviewEnded(dismissed) {
      if (!root.reopenAfterPreview) return
      root.reopenAfterPreview = false
      if (dismissed) {
        root.keepSettingsPage = root.settingsPath !== ""
        root.open()
      }
    }
  }

  function activate() {
    var row = root.rows[root.cursor]
    if (row === "tabs") setTab(["screensaver", "settings", "about"][( ["screensaver", "settings", "about"].indexOf(root.tab) + 1) % 3])
    else if (row.indexOf("about:") === 0) openLink(row)
    else if (row === "settings:tabs") root.setSettingsTab(root.settingsTab === "overlay" ? "performance" : "overlay")
    else if (row === "performance:prepare") { if (svc) svc.prepareSelected() }
    else if (row === "performance:clear") { if (svc) svc.clearPrepared() }
    else if (row === "enable") toggleOnIdle()
    else if (row === "delay") stepDelay(1)
    else if (row === "settings:back") closeSettings()
    else if (row === "settings:live") { if (svc) svc.setLivePreviews(!root.liveOn) }
    else if (row === "settings:photos") openPhotos()
    else if (row === "settings:videos") openVideos()
    else if (row === "overlay:reset") { if (svc) svc.resetOverlay() }
    else if (row === "settings:reset") { if (svc) svc.resetSettings(root.settingsPath) }
    else if (row.indexOf("set:") === 0) stepSetting(row.slice(4), 1, true)
    else if (row === "preview") preview()
    else choose(row)
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  onOpenedChanged: if (opened) {
    reopenAfterPreview = false
    // A settings page stays only for the panel coming back from a preview started on it.
    if (!keepSettingsPage) settingsPath = ""
    keepSettingsPage = false
    cursorActive = false
    // Where the cursor was last; or the chosen pack; or the top.
    var chosenRow = root.pack === "auto" ? "pack:auto" : "pack:" + root.pack
    openRow = rows.indexOf(lastRow) >= 0 ? lastRow : rows.indexOf(chosenRow) >= 0 ? chosenRow : rows[0]
    cursor = rows.indexOf(openRow)
    cursorRow = openRow
    // Show the chosen card immediately without resizing neighboring rows.
    cursorActive = true
    pointerSeen = false
    keyMoved = false
    root.setScroll(0)
    showChosen = true
    Qt.callLater(root.scrollToChosen)
    if (svc) svc.refresh()
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  Timer {
    id: startTimer
    interval: 250
    onTriggered: {
      // If the screensaver doesn't start (say, the session is locked), nothing comes back to reopen the panel for.
      if (!root.svc || root.svc.preview(root.previewPath) !== "starting") root.reopenAfterPreview = false
      else previewWatch.restart()
    }
  }

  Timer {
    id: previewWatch
    interval: 5000
    onTriggered: if (root.svc && !root.svc.running) root.reopenAfterPreview = false
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    iconComponent: Component {
      ShieldMark {
        fontFamily: root.fontFamily
      }
    }
    tooltipText: root.onIdle ? "ScreenSafe · on" : "ScreenSafe · off"
    dimmed: !root.onIdle
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.RightButton) root.toggleOnIdle()
      else root.toggle()
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(420))
    contentHeight: root.panelHeight()

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onMoveRequested: function(dx, dy) {
        root.keyMoved = true
        root.showChosen = false
        if (!root.cursorActive) { root.cursorActive = true; return }
        if (dy !== 0) {
          root.cursor = Math.max(0, Math.min(root.rows.length - 1, root.cursor + (dy > 0 ? 1 : -1)))
        } else if (dx !== 0 && root.rows[root.cursor] === "delay") {
          root.stepDelay(dx)
        } else if (dx !== 0 && root.rows[root.cursor] === "tabs") {
          root.setTab(["screensaver", "settings", "about"][( ["screensaver", "settings", "about"].indexOf(root.tab) + (dx > 0 ? 1 : 2)) % 3])
        } else if (dx !== 0 && root.rows[root.cursor] === "settings:tabs") {
          root.setSettingsTab(root.settingsTab === "overlay" ? "performance" : "overlay")
        } else if (dx !== 0 && root.rows[root.cursor].indexOf("set:") === 0) {
          root.stepSetting(root.rows[root.cursor].slice(4), dx > 0 ? 1 : -1, false)
        } else if (dx !== 0 && root.rows[root.cursor].indexOf("color:") === 0) {
          var parts = root.rows[root.cursor].split(":")
          root.setSettingValue(parts[1], ColorModel.adjust(root.settingsValues[parts[1]], parts[2], dx > 0 ? 1 : -1, root.svc.themeSwatches))
        } else if (dx < 0 && root.rows[root.cursor] === "settings:back") {
          root.closeSettings()
        } else if (dx > 0 && root.rows[root.cursor].indexOf("pack:") === 0) {
          root.openSettings(root.rows[root.cursor])
        }
      }
      onActivateRequested: if (root.cursorActive) root.activate()
      onCloseRequested: root.settingsPath !== "" ? root.closeSettings() : root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }
      onTextKey: function(t) {
        if (t === "p" || t === "P") root.preview()
        else if (t === " ") root.toggleOnIdle()
        else if ((t === "f" || t === "F") && root.cursorActive)
          root.toggleFavorite(root.rows[root.cursor])
        else if ((t === "s" || t === "S") && root.cursorActive)
          root.openSettings(root.rows[root.cursor])
      }

      // The title, pinned above the list.
      Column {
        id: header
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        bottomPadding: Style.space(10)
        spacing: Style.space(10)

        PanelHero {
          width: header.width
          title: "ScreenSafe"
          meta: root.statusText
          foreground: root.foreground
          fontFamily: root.fontFamily
          iconOpacity: root.onIdle ? 1.0 : 0.5
          iconComponent: Component {
            ShieldMark {
              width: Style.font.display * 0.95
              height: Style.font.display * 1.1
              fontFamily: root.fontFamily
            }
          }
          // When it starts, with − and + to change it; the arrow keys too, when the cursor is on it.
          trailingControl: Component {
            CursorSurface {
              id: delayControl
              hasCursor: root.at("delay")
              foreground: root.foreground
              implicitWidth: delayLayout.implicitWidth + Style.space(16)
              implicitHeight: delayLayout.implicitHeight + Style.space(8)

              MouseArea {
                id: delayHover
                anchors.fill: parent
                hoverEnabled: true
                onPositionChanged: function(mouse) { root.pointFrom(delayHover, mouse, "delay") }
              }

              PanelToolTip {
                visible: delayHover.containsMouse
                text: root.delayDetail
                fontFamily: root.fontFamily
              }

              RowLayout {
                id: delayLayout
                anchors.centerIn: parent
                spacing: Style.space(8)

                Text {
                  textFormat: Text.PlainText
                  text: root.delayText.toUpperCase()
                  color: root.dim
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.caption
                  font.bold: true
                  font.letterSpacing: 1.2
                  Layout.alignment: Qt.AlignVCenter
                }

                StepButton { glyph: "󰍴"; tip: "Sooner"; onStep: root.stepDelay(-1) }
                StepButton { glyph: "󰐕"; tip: "Later"; onStep: root.stepDelay(1) }
              }
            }
          }
        }

        Item {
          id: modeRow
          width: header.width
          readonly property bool stacked: tabRow.implicitWidth + enableRow.implicitWidth + Style.space(10) > width
          height: stacked ? tabRow.implicitHeight + enableRow.implicitHeight + Style.space(4) : Math.max(tabRow.implicitHeight, enableRow.implicitHeight)

          Row {
            id: tabRow
            anchors.left: parent.left
            y: modeRow.stacked ? 0 : (modeRow.height - implicitHeight) / 2
            spacing: Style.space(8)

            Repeater {
              model: [{ value: "screensaver", label: "Screensaver" }, { value: "settings", label: "Settings" }, { value: "about", label: "About" }]
              Button {
                required property var modelData
                text: modelData.label
                selected: root.tab === modelData.value
                hasCursor: root.at("tabs") && root.tab === modelData.value
                bordered: true
                foreground: root.foreground
                fontFamily: root.fontFamily
                onClicked: root.setTab(modelData.value)
                onHovered: function(on) { if (on) root.hoverPoint("tabs") }
              }
            }
          }

          // Whether ScreenSafe is the screensaver. The label names the setting; the switch shows its state.
          CursorSurface {
            id: enableRow
            anchors.right: parent.right
            y: modeRow.height - implicitHeight
            hasCursor: root.at("enable")
            foreground: root.foreground
            implicitWidth: enableLayout.implicitWidth + Style.space(16)
            implicitHeight: enableLayout.implicitHeight + Style.space(8)

            MouseArea {
              id: enableHover
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onPositionChanged: function(mouse) { root.pointFrom(enableHover, mouse, "enable") }
              onClicked: root.toggleOnIdle()
            }

            PanelToolTip {
              visible: enableHover.containsMouse
              text: "Show ScreenSafe instead of Omarchy's screensaver"
              fontFamily: root.fontFamily
            }

            RowLayout {
              id: enableLayout
              anchors.centerIn: parent
              spacing: Style.space(8)

              Text {
                textFormat: Text.PlainText
                text: "Enable"
                color: root.foreground
                font.family: root.fontFamily
                font.pixelSize: Style.font.body
                Layout.alignment: Qt.AlignVCenter
              }

              ToggleSwitch {
                checked: root.onIdle
                foreground: root.foreground
                Layout.alignment: Qt.AlignVCenter
                onHovered: function(on) { if (on) root.hoverPoint("enable") }
                onToggled: root.toggleOnIdle()
              }
            }
          }
        }

        PanelSeparator { foreground: root.foreground }
      }

      // About, in two halves: the mark, the name and a line on what ScreenSafe is, centered in the top half, and where
      // to go next, centered in the bottom half.
      Item {
        id: about
        visible: root.tab === "about"
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: header.bottom
        anchors.bottom: statusRow.top
        implicitHeight: 2 * Math.max(aboutTop.implicitHeight, aboutLinks.implicitHeight) + Style.space(20)

        Item {
          id: aboutTopHalf
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.top: parent.top
          height: parent.height / 2

          Column {
            id: aboutTop
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter

            ShieldMark {
              id: aboutMark
              anchors.horizontalCenter: parent.horizontalCenter
              width: Style.font.display * 5.2
              height: Style.font.display * 6.0
              fontFamily: root.fontFamily
            }

            Item { width: 1; height: Style.space(12) }

            Text {
              width: parent.width
              horizontalAlignment: Text.AlignHCenter
              text: "ScreenSafe"
              color: root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.display
              font.bold: true
            }

            Item { width: 1; height: Style.space(14) }

            Text {
              width: parent.width
              horizontalAlignment: Text.AlignHCenter
              wrapMode: Text.Wrap
              text: "Screensavers for Omarchy made of data, not code."
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              font.bold: true
            }
          }
        }

        Item {
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.bottom: parent.bottom
          height: parent.height / 2

          Column {
            id: aboutLinks
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.space(4)

            LinkRow { row: "about:project"; glyph: "󰊤"; title: "Project page"; detail: "Source, guides and releases" }
            LinkRow { row: "about:find"; glyph: "󰍉"; title: "Find a screensaver"; detail: "Packs others have made, approved and listed" }
            LinkRow { row: "about:make"; glyph: "󰏫"; title: "Make a screensaver"; detail: "A guide, and a skill for coding agents" }
            LinkRow { row: "about:share"; glyph: "󰒗"; title: "Share yours"; detail: "How to list a pack for others" }
          }
        }
      }

      // Back and preview stay fixed while the pack's options scroll beneath them.
      Item {
        id: settingsView
        visible: root.tab === "screensaver" && root.settingsPath !== ""
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: header.bottom
        anchors.bottom: footer.top

        Column {
          id: settingsHeader
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.top: parent.top
          spacing: Style.space(4)

          CursorSurface {
            id: backRow
            width: parent.width
            hasCursor: root.at("settings:back")
            foreground: root.foreground
            implicitHeight: backLayout.implicitHeight + Style.spacing.rowPaddingX
            onHasCursorChanged: if (hasCursor) root.revealInPage(backRow)

            MouseArea {
              id: backHover
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onPositionChanged: function(mouse) { root.pointFrom(backHover, mouse, "settings:back") }
              onClicked: root.closeSettings()
            }

            RowLayout {
              id: backLayout
              anchors.left: parent.left
              anchors.right: parent.right
              anchors.verticalCenter: parent.verticalCenter
              anchors.leftMargin: Style.space(10)
              anchors.rightMargin: Style.space(10)
              spacing: Style.space(10)

              Text {
                text: "󰁍"
                color: root.foreground
                font.family: root.fontFamily
                font.pixelSize: Style.font.heading
                Layout.alignment: Qt.AlignVCenter
              }

              RowText {
                Layout.fillWidth: true
                title: root.settingsName
                detail: "Settings · Esc goes back to the list"
              }
            }
          }

          // Reserve useful option space on short screens instead of clipping the live surface.
          ThumbView {
            id: pageThumbBox
            width: Math.min(parent.width, Math.round(height * root.screenAspect))
            anchors.horizontalCenter: parent.horizontalCenter
            height: Math.max(0, Math.min(Math.round(parent.width / root.screenAspect),
              settingsView.height - backRow.height - settingsHeader.spacing - colorDock.height - Style.space(root.pinnedColorDecl ? 100 : 144)))
            visible: height >= Style.space(32)
            thumb: root.pageThumb
          }
        }

        Flickable {
          id: pageScroller
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.top: settingsHeader.bottom
          anchors.topMargin: Style.space(4)
          anchors.bottom: colorDock.top
          anchors.bottomMargin: colorDock.visible ? Style.space(4) : 0
          contentWidth: width
          contentHeight: settingsPage.implicitHeight
          onContentHeightChanged: contentY = Math.max(0, Math.min(contentY, contentHeight - height))
          clip: true
          boundsBehavior: Flickable.StopAtBounds

          Column {
            id: settingsPage
            width: parent.width
            spacing: Style.space(4)

            CursorSurface {
              id: liveRow
              width: parent.width
              hasCursor: root.at("settings:live")
              foreground: root.foreground
              implicitHeight: liveLayout.implicitHeight + Style.spacing.rowPaddingX
              onHasCursorChanged: if (hasCursor) root.revealInPage(liveRow)

              MouseArea {
                id: liveHover
                anchors.fill: parent
                hoverEnabled: true
                onPositionChanged: function(mouse) { root.pointFrom(liveHover, mouse, "settings:live") }
                onClicked: if (root.svc) root.svc.setLivePreviews(!root.liveOn)
              }

              RowLayout {
                id: liveLayout
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                anchors.leftMargin: Style.space(10)
                anchors.rightMargin: Style.space(10)
                spacing: Style.space(10)

                RowText {
                  Layout.fillWidth: true
                  title: "Live preview"
                  detail: root.liveOn ? "Changes show as you make them" : "Off: a still picture, and Preview below"
                }

                ToggleSwitch {
                  checked: root.liveOn
                  foreground: root.foreground
                  Layout.alignment: Qt.AlignVCenter
                  onHovered: function(on) { if (on) root.hoverPoint("settings:live") }
                  onToggled: if (root.svc) root.svc.setLivePreviews(!root.liveOn)
                }
              }
            }

            // For a pack that shows photos: where they come from, and a way to add some.
            CursorSurface {
              id: photosRow
              visible: root.settingsPhotos >= 0
              width: parent.width
              hasCursor: root.at("settings:photos")
              foreground: root.foreground
              implicitHeight: visible ? photosLayout.implicitHeight + Style.spacing.rowPaddingX : 0
              onHasCursorChanged: if (hasCursor) root.revealInPage(photosRow)

              MouseArea {
                id: photosHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onPositionChanged: function(mouse) { root.pointFrom(photosHover, mouse, "settings:photos") }
                onClicked: root.openPhotos()
              }

              RowLayout {
                id: photosLayout
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                anchors.leftMargin: Style.space(10)
                anchors.rightMargin: Style.space(10)
                spacing: Style.space(10)

                RowText {
                  Layout.fillWidth: true
                  title: "Your photos"
                  detail: root.settingsPhotos === 0 ? "None yet: add some to ~/Pictures/Screensaver"
                    : root.settingsPhotos + (root.settingsPhotos === 1 ? " photo" : " photos") + " in ~/Pictures/Screensaver"
                }

                Text {
                  text: "Open folder 󰏌"
                  color: photosHover.containsMouse || photosRow.hasCursor ? root.foreground : root.dim
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.caption
                  Layout.alignment: Qt.AlignVCenter
                }
              }
            }

            // For a pack that shows videos: where they come from, and a way to add some.
            CursorSurface {
              id: videosRow
              visible: root.settingsVideos >= 0
              width: parent.width
              hasCursor: root.at("settings:videos")
              foreground: root.foreground
              implicitHeight: visible ? videosLayout.implicitHeight + Style.spacing.rowPaddingX : 0
              onHasCursorChanged: if (hasCursor) root.revealInPage(videosRow)

              MouseArea {
                id: videosHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onPositionChanged: function(mouse) { root.pointFrom(videosHover, mouse, "settings:videos") }
                onClicked: root.openVideos()
              }

              RowLayout {
                id: videosLayout
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                anchors.leftMargin: Style.space(10)
                anchors.rightMargin: Style.space(10)
                spacing: Style.space(10)

                RowText {
                  Layout.fillWidth: true
                  title: "Your videos"
                  detail: root.settingsVideos === 0 ? "None yet: import a clip into ~/Videos/Screensaver"
                    : root.settingsVideos + (root.settingsVideos === 1 ? " video" : " videos") + " in ~/Videos/Screensaver"
                }

                Text {
                  text: "Open folder 󰏌"
                  color: videosHover.containsMouse || videosRow.hasCursor ? root.foreground : root.dim
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.caption
                  Layout.alignment: Qt.AlignVCenter
                }
              }
            }

            PanelSeparator { foreground: root.foreground }

            Item { width: 1; height: Style.space(4) }

            Repeater {
              model: root.scrollingSettingsDecl
              SettingRow { required property var modelData; decl: modelData; width: settingsPage.width }
            }

            PanelSeparator { foreground: root.foreground }

            CursorSurface {
              id: resetRow
              width: parent.width
              hasCursor: root.at("settings:reset")
              foreground: root.foreground
              implicitHeight: resetLayout.implicitHeight + Style.spacing.rowPaddingX
              onHasCursorChanged: if (hasCursor) root.revealInPage(resetRow)

              MouseArea {
                id: resetHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onPositionChanged: function(mouse) { root.pointFrom(resetHover, mouse, "settings:reset") }
                onClicked: if (root.svc) root.svc.resetSettings(root.settingsPath)
              }

              RowLayout {
                id: resetLayout
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                anchors.leftMargin: Style.space(10)
                anchors.rightMargin: Style.space(10)
                spacing: Style.space(10)

                Text {
                  text: "󰑓"
                  color: root.foreground
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.heading
                  Layout.alignment: Qt.AlignVCenter
                }

                RowText {
                  Layout.fillWidth: true
                  title: "Reset to defaults"
                  detail: "As the pack comes"
                }
              }
            }
          }
        }
        Flickable {
          id: colorDock
          visible: root.pinnedColorDecl !== null
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.bottom: parent.bottom
          height: visible ? Math.min(colorDockPage.implicitHeight, Math.max(0, settingsView.height - backRow.height - Style.space(80))) : 0
          contentWidth: width
          contentHeight: colorDockPage.implicitHeight
          onContentHeightChanged: contentY = Math.max(0, Math.min(contentY, contentHeight - height))
          onHeightChanged: contentY = Math.max(0, Math.min(contentY, contentHeight - height))
          clip: true
          boundsBehavior: Flickable.StopAtBounds
          Column {
            id: colorDockPage
            width: parent.width
            spacing: Style.space(4)
            PanelSeparator { foreground: root.foreground }
            Repeater {
              model: root.pinnedColorDecl ? [root.pinnedColorDecl] : []
              SettingRow { required property var modelData; decl: modelData; width: colorDockPage.width; compactColor: true }
            }
          }
        }
      }

      Column {
        id: overlayPositionDock
        visible: root.overlayPageActive && root.overlayPositionDecl !== null
        height: visible ? implicitHeight : 0
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: footer.top
        spacing: Style.space(4)
        PanelSeparator { foreground: root.foreground }
        Repeater {
          model: root.overlayPositionDecl ? [root.overlayPositionDecl] : []
          SettingRow { required property var modelData; decl: modelData; width: overlayPositionDock.width }
        }
      }

      Row {
        id: settingsTabs
        visible: root.tab === "settings"
        anchors.top: header.bottom
        anchors.topMargin: Style.space(8)
        anchors.horizontalCenter: parent.horizontalCenter
        spacing: Style.space(8)
        Repeater {
          model: [{ value: "overlay", label: "Overlay" }, { value: "performance", label: "Performance" }]
          Button {
            required property var modelData
            text: modelData.label
            selected: root.settingsTab === modelData.value
            hasCursor: root.at("settings:tabs") && selected
            foreground: root.foreground
            fontFamily: root.fontFamily
            bordered: true
            onClicked: root.setSettingsTab(modelData.value)
            onHovered: function(on) { if (on) root.hoverPoint("settings:tabs") }
          }
        }
      }

      Flickable {
        id: performanceScroller
        visible: root.performancePageActive
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: settingsTabs.bottom
        anchors.topMargin: Style.space(8)
        anchors.bottom: footer.top
        contentWidth: width
        contentHeight: performancePage.implicitHeight
        boundsBehavior: Flickable.StopAtBounds
        clip: true
        Column {
          id: performancePage
          width: parent.width
          spacing: Style.space(8)
          PanelSectionHeader { text: "PERFORMANCE"; foreground: root.foreground; fontFamily: root.fontFamily }
          Text {
            width: parent.width
            text: "Prepared loops can reduce power for demanding screensavers. Simple effects may use less power live. Preparation runs on external power. Live clocks, photos, videos and the overlay use normal playback."
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
          }
          Repeater {
            model: PerformanceModel.declarations
            SettingRow { required property var modelData; width: performancePage.width; decl: modelData }
          }
          Text {
            width: parent.width
            text: root.svc ? root.svc.performanceDetail : ""
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
          }
          Repeater {
            model: [{ row: "performance:prepare", title: root.svc && root.svc.preparing ? "Cancel preparation" : "Prepare selected screensaver", detail: "Build a local loop for the current theme and settings" }, { row: "performance:clear", title: "Clear prepared loops", detail: "Keep screensavers and their settings" }]
            CursorSurface {
              required property var modelData
              width: performancePage.width
              implicitHeight: actionText.implicitHeight + Style.spacing.rowPaddingX
              hasCursor: root.at(modelData.row)
              foreground: root.foreground
              onHasCursorChanged: if (hasCursor) root.revealInPage(this)
              RowText { id: actionText; anchors.centerIn: parent; width: parent.width - Style.space(20); title: modelData.title; detail: modelData.detail }
              MouseArea {
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onPositionChanged: function(mouse) { root.pointFrom(this, mouse, parent.modelData.row) }
                onClicked: if (root.svc) { if (parent.modelData.row === "performance:prepare") root.svc.prepareSelected(); else root.svc.clearPrepared() }
              }
            }
          }
        }
      }

      Flickable {
        id: overlayScroller
        visible: root.overlayPageActive
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: settingsTabs.bottom
        anchors.topMargin: Style.space(8)
        anchors.bottom: overlayPositionDock.top
        anchors.bottomMargin: Style.space(4)
        contentWidth: width
        contentHeight: overlayPage.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        Column {
          id: overlayPage
          width: parent.width
          spacing: Style.space(8)
          PanelSectionHeader { text: "OVERLAY"; foreground: root.foreground; fontFamily: root.fontFamily }
          Text {
            width: parent.width
            text: "Show a clock and optional system readings over any screensaver."
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
          }
          Repeater {
            model: root.overlayScrollingDecl
            SettingRow { required property var modelData; width: overlayPage.width; decl: modelData }
          }
          CursorSurface {
            id: overlayResetRow
            width: parent.width
            hasCursor: root.at("overlay:reset")
            foreground: root.foreground
            implicitHeight: overlayReset.implicitHeight + Style.spacing.rowPaddingX
            onHasCursorChanged: if (hasCursor) root.revealInPage(overlayResetRow)
            RowText { id: overlayReset; anchors.centerIn: parent; width: parent.width - Style.space(20); title: "Reset overlay"; detail: "Turn off and restore defaults" }
            MouseArea { id: overlayResetMouse; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onPositionChanged: function(mouse) { root.pointFrom(overlayResetMouse, mouse, "overlay:reset") }; onClicked: if (root.svc) root.svc.resetOverlay() }
          }
        }
      }

      Flickable {
        id: scroller
        visible: root.tab === "screensaver" && root.settingsPath === ""
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: header.bottom
        anchors.bottom: footer.top
        contentWidth: width
        contentHeight: column.implicitHeight
        onContentHeightChanged: {
          if (root.showChosen) root.scrollToChosen()
          else if (root.revealTarget && root.revealTarget.hasCursor) root.reveal(root.revealTarget)
          // A list that got shorter while scrolled to its end stays scrolled to its end, not past it.
          else if (contentY > Math.max(0, contentHeight - height)) root.setScroll(Math.max(0, contentHeight - height))
        }
        onHeightChanged: if (root.showChosen) root.scrollToChosen()
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        Column {
          id: column
          width: parent.width
          spacing: Style.space(10)

          Column {
            id: packList
            width: parent.width
            spacing: Style.space(4)

            PanelSectionHeader {
              text: "PACK"
              foreground: root.foreground
              fontFamily: root.fontFamily
            }

            ChoiceRow {
              width: column.width
              row: "pack:auto"
              // Ticked when chosen, and when the pack that was chosen isn't available: Automatic's is shown then.
              chosen: root.pack === "auto" || (!!root.svc && root.packs.length > 0 && !root.svc.listed(root.pack))
              usable: true
              label: "Automatic"
              detail: root.svc && root.svc.currentPackName ? "Now " + root.svc.currentPackName : "The theme's pack, or Omarchy"
              info: "Shows the current theme's pack when it has one, and the Omarchy pack otherwise."
            }

            Repeater {
              model: root.favoritePacks
              PackRow { required property var modelData; pack: modelData }
            }

            PanelSeparator {
              foreground: root.foreground
              visible: root.favoritePacks.length > 0 && root.otherPacks.length > 0
            }

            Repeater {
              model: root.otherPacks
              PackRow { required property var modelData; pack: modelData }
            }
          }
        }
      }

      // Anything in the way, pinned to the bottom on either tab: only while there's something to say.
      Column {
        id: statusRow
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        visible: root.alertText !== ""
        height: visible ? implicitHeight : 0
        topPadding: Style.space(10)
        spacing: Style.space(10)

        PanelSeparator { foreground: root.foreground }

        RowLayout {
          width: parent.width
          spacing: Style.space(10)

          Text {
            text: "󰀦"
            color: root.bar ? root.bar.urgent : Color.urgent
            font.family: root.fontFamily
            font.pixelSize: Style.font.heading
            Layout.alignment: Qt.AlignVCenter
            Layout.leftMargin: Style.space(10)
          }

          Text {
            textFormat: Text.PlainText
            Layout.fillWidth: true
            Layout.rightMargin: Style.space(10)
            text: root.alertText
            wrapMode: Text.Wrap
            color: root.foreground
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
            Layout.alignment: Qt.AlignVCenter
          }
        }
      }

      // Preview, pinned below the list.
      Column {
        id: footer
        visible: root.overlayPageActive || (root.tab === "screensaver" && !(root.settingsPath !== "" && root.liveOn))
        height: visible ? implicitHeight : 0
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: statusRow.top
        topPadding: Style.space(10)
        spacing: Style.space(10)

        PanelSeparator { foreground: root.foreground }

        CursorSurface {
          id: previewRow
          width: parent.width
          hasCursor: root.at("preview")
          foreground: root.foreground
          implicitHeight: previewLayout.implicitHeight + Style.spacing.rowPaddingX

          MouseArea {
            id: previewHover
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onPositionChanged: function(mouse) { root.pointFrom(previewHover, mouse, "preview") }
            onClicked: root.preview()
          }

          RowLayout {
            id: previewLayout
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            anchors.leftMargin: Style.space(10)
            anchors.rightMargin: Style.space(10)
            spacing: Style.space(8)

            Text {
              text: "󰐊"
              color: root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.heading
              Layout.alignment: Qt.AlignVCenter
            }

            RowText {
              Layout.fillWidth: true
              title: root.settingsPath !== "" ? "Preview " + root.settingsName : "Preview"
              detail: "Any key, click or mouse movement closes it"
            }
          }
        }
      }
    }
  }

  // A link in About: opens the project's page in the browser.
  component LinkRow: CursorSurface {
    id: linkRow
    property string row
    property string glyph
    property string title
    property string detail

    width: parent ? parent.width : 0
    hasCursor: root.at(row)
    foreground: root.foreground
    implicitHeight: linkLayout.implicitHeight + Style.spacing.rowPaddingX

    MouseArea {
      id: linkHover
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onPositionChanged: function(mouse) { root.pointFrom(linkHover, mouse, linkRow.row) }
      onClicked: root.openLink(linkRow.row)
    }

    RowLayout {
      id: linkLayout
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(10)
      spacing: Style.space(10)

      Text {
        text: linkRow.glyph
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.heading
        Layout.alignment: Qt.AlignVCenter
      }

      RowText {
        Layout.fillWidth: true
        title: linkRow.title
        detail: linkRow.detail
      }

      Text {
        text: "󰏌"
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        Layout.alignment: Qt.AlignVCenter
      }
    }
  }

  // A row's title with a dimmer line under it.
  component RowText: ColumnLayout {
    property string title
    property string detail
    property bool warn: false
    spacing: Style.space(1)

    Text {
      textFormat: Text.PlainText
      Layout.fillWidth: true
      text: parent.title
      color: root.foreground
      font.family: root.fontFamily
      font.pixelSize: Style.font.body
      elide: Text.ElideRight
    }

    Text {
      textFormat: Text.PlainText
      Layout.fillWidth: true
      text: parent.detail
      color: parent.warn ? (root.bar ? root.bar.urgent : Color.urgent) : root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
      elide: Text.ElideRight
    }
  }

  // One pack from the catalog, as a choice.
  component PackRow: ChoiceRow {
    property var pack
    width: column.width
    row: "pack:" + pack.path
    chosen: root.pack === pack.path
    usable: pack.ok === true
    starrable: true
    starred: root.isFavorite(pack.path)
    label: pack.name
    detail: root.packDetail(pack)
    info: root.packInfo(pack)
  }

  // One choice, ticked when it's the one chosen; a pack that can't be used is dimmed and can't be chosen. A pack
  // can be starred, with its star or the "f" key, to list it first. Every card keeps its thumbnail
  // and description visible at a stable height.
  component ChoiceRow: CursorSurface {
    id: choiceRow
    property string row
    property bool chosen
    property bool usable
    property bool starrable: false
    property bool starred: false
    property string label
    property string detail
    property string info

    // The thumbnail: the service's "pixels W H <base64 RGB>", "" while it's being drawn, or "none".
    readonly property string thumb: !usable ? "none"
      : root.svc ? root.svc.thumbnailOf(row.slice(5), root.thumbSize) : "none"
    // Every card is expanded, but only nearby thumbnails need decoding. Scrolling doesn't resize cards.
    readonly property bool thumbWanted: {
      var top = packList.y + choiceRow.y - scroller.contentY
      return root.opened && root.tab === "screensaver" && root.settingsPath === ""
        && top + height >= -height && top <= scroller.height + height
    }
    // Check cached thumbnails when the panel opens; the service serializes and deduplicates work.
    function askForThumb() { if (thumbWanted && usable && root.svc) root.svc.requestThumbnail(row.slice(5), root.thumbSize) }
    onThumbChanged: if (thumb === "") askForThumb()
    onThumbWantedChanged: askForThumb()

    hasCursor: root.at(row)
    onHasCursorChanged: if (hasCursor) {
      root.reveal(choiceRow)
    }
    // Keep keyboard navigation in view when the card layout settles.
    onHeightChanged: if (hasCursor) root.reveal(choiceRow)
    // A row made anew where a starred or unstarred pack now sits may already have the cursor.
    Component.onCompleted: {
      askForThumb()
      if (hasCursor && root.keyMoved) Qt.callLater(function() { root.reveal(choiceRow) })
    }
    Connections {
      target: root
      function onOpenedChanged() { if (root.opened) choiceRow.askForThumb() }
    }
    current: chosen
    foreground: root.foreground
    opacity: usable ? 1.0 : 0.55
    implicitHeight: choiceColumn.implicitHeight + Style.spacing.rowPaddingX

    MouseArea {
      id: rowHover
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: choiceRow.usable ? Qt.PointingHandCursor : Qt.ArrowCursor
      onPositionChanged: function(mouse) { root.pointFrom(rowHover, mouse, choiceRow.row) }
      onClicked: {
        root.point(choiceRow.row)
        root.choose(choiceRow.row)
        root.scrollTo(choiceRow)
      }
    }

    ColumnLayout {
      id: choiceColumn
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(10)
      spacing: Style.space(8)

      RowLayout {
        id: choiceLayout
        Layout.fillWidth: true
        spacing: Style.space(8)

        RowText {
          Layout.fillWidth: true
          title: choiceRow.label
          detail: choiceRow.detail
        }

        // The gear, for a pack with settings: opens its settings page. Packs without keep the space empty, so the
        // stars line up.
        Text {
          id: gearMark
          readonly property bool offered: choiceRow.starrable && root.hasSettings(choiceRow.row.slice(5))
          text: offered ? "󰒓" : ""
          Layout.preferredWidth: Style.font.body
          horizontalAlignment: Text.AlignHCenter
          color: gearMouse.containsMouse ? root.foreground : root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          Layout.alignment: Qt.AlignVCenter

          MouseArea {
            id: gearMouse
            anchors.fill: parent
            anchors.margins: -Style.space(4)
            enabled: gearMark.offered
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onPositionChanged: function(mouse) { root.pointFrom(gearMouse, mouse, choiceRow.row) }
            onClicked: root.openSettings(choiceRow.row)
          }

          PanelToolTip {
            visible: gearMouse.containsMouse
            text: "Settings"
            fontFamily: root.fontFamily
          }
        }

        // The star: filled for a favorite; outlined, and dim until pointed at, otherwise. Automatic keeps the space
        // empty so the marks line up.
        Text {
          id: starMark
          text: choiceRow.starrable ? (choiceRow.starred ? "󰓎" : "󰓒") : ""
          Layout.preferredWidth: Style.font.body
          horizontalAlignment: Text.AlignHCenter
          color: choiceRow.starred || starMouse.containsMouse ? root.foreground : root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          Layout.alignment: Qt.AlignVCenter

          MouseArea {
            id: starMouse
            anchors.fill: parent
            anchors.margins: -Style.space(4)
            enabled: choiceRow.starrable
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onPositionChanged: function(mouse) { root.pointFrom(starMouse, mouse, choiceRow.row) }
            onClicked: root.toggleFavorite(choiceRow.row)
          }

          PanelToolTip {
            visible: starMouse.containsMouse
            text: choiceRow.starred ? "Remove from favorites" : "Add to favorites"
            fontFamily: root.fontFamily
          }
        }

        Text {
          text: choiceRow.chosen ? "󰄬" : ""
          Layout.preferredWidth: Style.font.heading
          horizontalAlignment: Text.AlignHCenter
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.heading
          Layout.alignment: Qt.AlignVCenter
        }
      }

      // A fixed preview on the left and description on the right.
      RowLayout {
        id: expansion
        Layout.fillWidth: true
        spacing: Style.space(10)
        // Fixed thumbnail dimensions keep every card stable while previews load.
        readonly property real pitch: choiceLayout.implicitHeight + Style.spacing.rowPaddingX + packList.spacing
        readonly property int rowsTall: Math.max(1, Math.round((choiceColumn.width * 0.5 / root.screenAspect + choiceColumn.spacing) / pitch))
        readonly property real thumbHeight: rowsTall * pitch - choiceColumn.spacing
        readonly property real thumbWidth: Math.round(thumbHeight * root.screenAspect)

        ThumbView {
          Layout.preferredWidth: expansion.thumbWidth
          Layout.preferredHeight: expansion.thumbHeight
          Layout.alignment: Qt.AlignTop
          thumb: choiceRow.thumb
        }

        Text {
          id: infoText
          Layout.fillWidth: true
          Layout.preferredHeight: expansion.thumbHeight
          Layout.maximumHeight: expansion.thumbHeight
          Layout.alignment: Qt.AlignTop
          textFormat: Text.PlainText
          text: choiceRow.info
          wrapMode: Text.Wrap
          elide: Text.ElideRight
          maximumLineCount: Math.max(1, Math.floor(expansion.thumbHeight / Math.max(1, fontMetrics.lineSpacing)))
          color: root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption

          FontMetrics {
            id: fontMetrics
            font: infoText.font
          }
        }
      }
    }
  }

  // A thumbnail, from the service's plain pixels ("pixels W H <base64 RGB>"), "" while it's drawn, or "none". The
  // service has checked the size and the length of the pixels; the panel puts its own binary PPM header before them,
  // so Qt's PPM reader is the only image reader that runs, and loads it off the shell's main thread.
  component ThumbView: Rectangle {
    id: thumbView
    property string thumb
    color: "transparent"
    border.color: root.dim
    border.width: 1

    Image {
      id: thumbImage
      readonly property string pixels: thumbView.thumb.indexOf("pixels ") === 0 ? thumbView.thumb : ""
      readonly property var parts: pixels.split(" ")
      readonly property int pw: parts.length === 4 ? Number(parts[1]) : 0
      readonly property int ph: parts.length === 4 ? Number(parts[2]) : 0
      visible: pw > 0 && ph > 0
      anchors.fill: parent
      anchors.margins: 1
      fillMode: Image.PreserveAspectFit
      asynchronous: true
      cache: false
      smooth: true
      source: {
        if (pw <= 0 || ph <= 0 || pw > 480 || ph > 480 || parts[3].length !== pw * ph * 4) return ""
        var header = "P6\n" + pw + " " + ph + "\n"
        // Align the header to complete base64 groups so the already encoded RGB can follow it unchanged.
        var padding = (3 - (header.length + 4) % 3) % 3
        for (var i = 0; i < padding; i++) header += " "
        header += "255\n"
        var bytes = []
        for (var j = 0; j < header.length; j++) bytes.push(header.charCodeAt(j))
        return "data:image/x-portable-pixmap;base64," + Qt.btoa(bytes) + parts[3]
      }
    }

    Text {
      anchors.centerIn: parent
      visible: thumbView.thumb === "" || thumbView.thumb === "none"
      text: thumbView.thumb === "" ? "Drawing…" : "No preview"
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
    }
  }

  // One setting on a pack's settings page: its label, and the control for its kind.
  component SettingRow: CursorSurface {
    id: settingRow
    property var decl
    property bool compactColor: false
    readonly property string row: "set:" + decl.name
    readonly property var value: (root.performancePageActive && root.svc ? root.svc.performanceValues : root.overlayPageActive && root.svc ? root.svc.overlayValues : root.settingsValues)[decl.name]
    visible: root.overlayPageActive || root.performancePageActive || root.settingActive(decl)
    hasCursor: root.at(row)
    foreground: root.foreground
    implicitHeight: (decl.kind === "color" ? colorLayout.implicitHeight : settingLayout.implicitHeight) + Style.spacing.rowPaddingX
    onHasCursorChanged: if (hasCursor) root.revealInPage(settingRow)

    MouseArea {
      id: settingHover
      anchors.fill: parent
      hoverEnabled: true
      onPositionChanged: function(mouse) { root.pointFrom(settingHover, mouse, settingRow.row) }
      onClicked: if (settingRow.decl.kind === "toggle") root.stepSetting(settingRow.decl.name, 1, true)
    }

    RowLayout {
      id: settingLayout
      visible: settingRow.decl.kind !== "color"
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(10)
      spacing: Style.space(10)

      Text {
        textFormat: Text.PlainText
        Layout.fillWidth: true
        text: settingRow.decl.label
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        elide: Text.ElideRight
        Layout.alignment: Qt.AlignVCenter
      }

      ToggleSwitch {
        visible: settingRow.decl.kind === "toggle"
        checked: settingRow.value === 1
        foreground: root.foreground
        Layout.alignment: Qt.AlignVCenter
        onHovered: function(on) { if (on) root.hoverPoint(settingRow.row) }
        onToggled: root.stepSetting(settingRow.decl.name, 1, true)
      }

      // A choice: its options side by side, the chosen one bright and marked.
      Row {
        visible: settingRow.decl.kind === "choice"
        spacing: Style.space(6)
        Layout.alignment: Qt.AlignVCenter

        Repeater {
          model: settingRow.decl.kind === "choice" ? settingRow.decl.choices : []
          Text {
            required property var modelData
            required property int index
            readonly property bool picked: settingRow.decl.values[index] === settingRow.value
            textFormat: Text.PlainText
            text: picked ? "[" + modelData + "]" : modelData
            color: picked || optionMouse.containsMouse ? root.foreground : root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
            font.bold: picked

            MouseArea {
              id: optionMouse
              anchors.fill: parent
              anchors.margins: -Style.space(3)
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onPositionChanged: function(mouse) { root.pointFrom(optionMouse, mouse, settingRow.row) }
              onClicked: root.setSettingValue(settingRow.decl.name, settingRow.decl.values[index])
            }
          }
        }
      }

      Grid {
        visible: root.overlayPageActive && settingRow.decl.name === "position"
        columns: 3
        spacing: Style.space(3)
        Layout.alignment: Qt.AlignVCenter
        Repeater {
          model: parent.visible ? ["↖", "↑", "↗", "←", "·", "→", "↙", "↓", "↘"] : []
          Button {
            required property var modelData
            required property int index
            property bool pointerOn: false
            text: modelData
            selected: settingRow.value === index
            hasCursor: root.at(settingRow.row) && selected
            bordered: true
            foreground: root.foreground
            fontFamily: root.fontFamily
            onClicked: root.setSettingValue("position", index)
            onHovered: function(on) { pointerOn = on; if (on) root.hoverPoint(settingRow.row) }
            PanelToolTip { visible: parent.pointerOn; text: OverlayModel.positions[parent.index]; fontFamily: root.fontFamily }
          }
        }
      }

      // A range: its value between steps down and up.
      RowLayout {
        visible: settingRow.decl.kind === "range" && !(root.overlayPageActive && settingRow.decl.name === "position")
        spacing: Style.space(8)
        Layout.alignment: Qt.AlignVCenter

        SettingStep { glyph: "−"; row: settingRow.row; onStep: root.stepSetting(settingRow.decl.name, -1, false) }
        Text {
          textFormat: Text.PlainText
          text: settingRow.decl.kind === "range" ? root.settingValueText(settingRow.decl) : ""
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
          Layout.alignment: Qt.AlignVCenter
        }
        SettingStep { glyph: "+"; row: settingRow.row; onStep: root.stepSetting(settingRow.decl.name, 1, false) }
      }
    }
    Column {
      id: colorLayout
      visible: settingRow.decl.kind === "color"
      anchors.left: parent.left; anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(10); anchors.rightMargin: Style.space(10)
      spacing: Style.space(8)
      Text { text: settingRow.decl.label; textFormat: Text.PlainText; color: root.foreground; font.family: root.fontFamily; font.pixelSize: Style.font.body }
      Loader {
        active: settingRow.decl.kind === "color" && settingRow.visible
        width: parent.width
        height: item ? item.implicitHeight : 0
        sourceComponent: Component {
          ScreenColorPicker {
            // A page change can clear its values before the old delegate is destroyed.
            value: ColorModel.valid(settingRow.value) ? settingRow.value : 0
            selected: settingRow.hasCursor
            compact: settingRow.compactColor
            swatches: root.svc ? root.svc.themeSwatches : []
            foreground: root.foreground; fontFamily: root.fontFamily
            cursorChannel: root.at("color:" + settingRow.decl.name + ":h") ? "h" : root.at("color:" + settingRow.decl.name + ":s") ? "s" : root.at("color:" + settingRow.decl.name + ":v") ? "v" : ""
            onPicked: function(selection) { root.setSettingValue(settingRow.decl.name, selection) }
            onPointing: function(area, mouse, channel) { root.pointFrom(area, mouse, channel === "" ? settingRow.row : "color:" + settingRow.decl.name + ":" + channel) }
            onChannelActivated: function(channel) { root.point(channel === "" ? settingRow.row : "color:" + settingRow.decl.name + ":" + channel) }
            onRevealRequested: function(item) { root.revealInPage(item) }
          }
        }
      }
    }
  }

  // A step down or up on a setting's range.
  component SettingStep: Text {
    id: settingStep
    property string glyph
    property string row
    signal step()
    text: glyph
    color: settingStepMouse.containsMouse ? root.foreground : root.dim
    font.family: root.fontFamily
    font.pixelSize: Style.font.heading
    Layout.alignment: Qt.AlignVCenter

    MouseArea {
      id: settingStepMouse
      anchors.fill: parent
      anchors.margins: -Style.space(4)
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onPositionChanged: function(mouse) { root.pointFrom(settingStepMouse, mouse, settingStep.row) }
      onClicked: settingStep.step()
    }
  }

  // A small button that steps the idle time.
  component StepButton: Text {
    id: stepButton
    property string glyph
    property string tip
    signal step()

    text: glyph
    color: stepMouse.containsMouse ? root.foreground : root.dim
    font.family: root.fontFamily
    font.pixelSize: Style.font.heading
    Layout.alignment: Qt.AlignVCenter

    MouseArea {
      id: stepMouse
      anchors.fill: parent
      anchors.margins: -Style.space(4)
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onPositionChanged: function(mouse) { root.pointFrom(stepMouse, mouse, "delay") }
      onClicked: stepButton.step()
    }

    PanelToolTip {
      visible: stepMouse.containsMouse
      text: stepButton.tip
      fontFamily: root.fontFamily
    }
  }
}
