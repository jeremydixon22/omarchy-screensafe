import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import qs.Commons
import qs.Commons as Commons
import qs.Ui

// The bar icon and its panel: whether ScreenSafe is the screensaver, which pack it shows, and a preview. The
// service does the work; this is a handle on it. Every string from a pack is shown as plain text. The row the
// cursor rests on opens to show a thumbnail of the pack beside its description; the chosen pack is ticked. Starred
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

  // Two tabs: the screensaver's settings, and About. The panel reopens on the one it was left on, so someone
  // following About's links comes back to them.
  property string tab: "screensaver"
  function setTab(t) {
    if (t === root.tab) return
    root.tab = t
    root.expandedRow = ""
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
    if (root.tab === "about") return ["delay", "tabs", "enable", "about:project", "about:find", "about:make", "about:share"]
    if (root.settingsPath !== "") {
      var page = ["delay", "tabs", "enable", "settings:back", "settings:live"]
      for (var s = 0; s < root.settingsDecl.length; s++) page.push("set:" + root.settingsDecl[s].name)
      page.push("settings:reset")
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
    expandTimer.restart()
  }
  onRowsChanged: if (cursorRow !== "" && rows.indexOf(cursorRow) >= 0) cursor = rows.indexOf(cursorRow)
  function at(row) { return root.cursorActive && root.rows[root.cursor] === row }
  // Whether the cursor last moved by keyboard. Only then does the list scroll to follow it: a row the pointer is on
  // is already in view, and scrolling under the pointer would hide that the list goes on.
  property bool keyMoved: false
  // The tabs and the switch report hovering on their own, sometimes as the panel opens under a resting pointer. That
  // moves the cursor only once the pointer has moved in the panel.
  function hoverPoint(row) { if (root.pointerSeen) root.point(row) }
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
  // open and the box is wholly in view; the thumbnail stays beneath it, for the moment before it shows.
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
    if (!root.liveWanted) { root.svc.stopLive(); return }
    var top = pageThumbBox.mapToItem(pageScroller, 0, 0).y
    if (top < 0 || top + pageThumbBox.height > pageScroller.height) { root.svc.stopLive(); return }
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
  // Keeps a row of the settings page in view.
  function revealInPage(item) {
    var top = item.mapToItem(settingsPage, 0, 0).y
    var bottom = top + item.height
    var view = pageScroller.height
    if (bottom > pageScroller.contentY + view) pageScroller.contentY = Math.min(Math.max(0, settingsPage.implicitHeight - view), bottom - view)
    if (top < pageScroller.contentY) pageScroller.contentY = Math.max(0, top)
  }

  // The pack row that's open, showing its thumbnail and description: the one the cursor rests on, after a moment.
  property string expandedRow: ""
  onCursorActiveChanged: {
    if (cursorActive) lastRow = rows[cursor] || ""
    expandTimer.restart()
  }
  Timer {
    id: expandTimer
    interval: 250
    onTriggered: {
      var row = root.cursorActive ? (root.rows[root.cursor] || "") : ""
      root.expandedRow = row.indexOf("pack:") === 0 ? row : ""
    }
  }
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
  // Keeps the cursor's row in view, as it moves and as rows open and close around it: the list scrolls only when the
  // row isn't wholly in view, and then just far enough to show it and a little of what's beyond, so the list never
  // looks as if it ends there when it doesn't.
  //
  // The cursor's row is anchored: it stays where it is on screen however the rows around it open and close, and only
  // then is the list scrolled, if it must be. When the pointer chose the row, never so far that the row leaves the
  // pointer. The scroll is worked out afresh from the anchor each time, so rows opening and closing one after the
  // other don't add up to a jump.
  property Item revealTarget: null
  property real anchorOffset: NaN      // the cursor's row's top, from the top of the list's view
  property bool adjusting: false       // the list is being scrolled here, not by the user
  function reveal(item) {
    root.revealTarget = item
    scrollTo(item)
  }
  function anchor(item) {
    root.anchorOffset = item.mapToItem(column, 0, 0).y - scroller.contentY
  }
  function setScroll(y) {
    root.adjusting = true
    scroller.contentY = y
    root.adjusting = false
  }
  function scrollTo(item) {
    var peek = Style.space(28)
    var top = item.mapToItem(column, 0, 0).y
    var bottom = top + item.height
    var view = scroller.height
    var most = Math.max(0, scroller.contentHeight - view)
    var anchored = item === root.revealTarget && !isNaN(root.anchorOffset)
    var byPointer = anchored && !root.keyMoved && root.pointerSeen
    var to = anchored ? top - root.anchorOffset : scroller.contentY
    if (bottom > to + view) to = bottom + peek - view
    if (top < to) to = top - peek
    if (byPointer) {
      // Where the pointer is in the list's view; the content under it is to + that.
      var at = scroller.mapFromGlobal(root.lastPointer.x, root.lastPointer.y).y
      if (at >= 0 && at <= view) to = Math.max(top - at + 1, Math.min(bottom - at - 1, to))
    }
    // The anchor keeps where the row should be; a list briefly too short to scroll there (as one row closes before
    // the next opens) doesn't move it.
    if (anchored) root.anchorOffset = top - to
    to = Math.max(0, Math.min(most, to))
    if (to !== scroller.contentY) root.setScroll(to)
  }
  // The panel's height. When the list doesn't fit, its bottom edge cuts through a row rather than falling between
  // two, so a list that goes on never looks complete.
  // The panel keeps that height wherever the cursor is: a row the cursor holds open is measured as if it were closed.
  function panelHeight() {
    var kids = packList.children
    var extra = 0
    for (var k = 0; k < kids.length; k++) if (kids[k].visible && kids[k].openHeight > 0) extra += kids[k].openHeight
    var listHeight = column.implicitHeight - extra
    var bottom = statusRow.visible ? statusRow.implicitHeight : 0
    var content = Math.max(listHeight + footer.implicitHeight, about.implicitHeight) + bottom
    var full = panel.fittedContentHeight(header.implicitHeight + content, Style.space(620))
    if (listHeight + footer.implicitHeight < about.implicitHeight) return full
    var shown = full - panel.verticalContentInset - header.implicitHeight - footer.implicitHeight - bottom
    if (shown >= listHeight - 0.5) return full
    var y = shown - packList.y
    var prev = null
    var shift = 0
    for (var i = 0; i < kids.length; i++) {
      var c = kids[i]
      if (!c.visible || c.height <= 0) continue
      var opened = c.openHeight > 0 ? c.openHeight : 0
      var top = c.y - shift
      var height = c.height - opened
      shift += opened
      if (y >= top + height + packList.spacing) { prev = { y: top, height: height }; continue }
      var cut = y - top
      // A separator is too thin to cut through; the row above it is cut instead.
      var thin = height < Style.space(16)
      if (!thin && cut >= height * 0.3 && cut <= height * 0.7) return full
      if (!thin && cut > height * 0.7) return full - (cut - height * 0.5)
      return prev ? full - (y - (prev.y + prev.height * 0.5)) : full
    }
    return full
  }

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
  readonly property string settingsName: {
    for (var i = 0; i < root.packs.length; i++) if (root.packs[i].path === root.settingsPath) return root.packs[i].name
    return ""
  }
  function hasSettings(path) { return !!root.svc && root.svc.declarationsOf(path).length > 0 }

  function openSettings(row) {
    if (row.indexOf("pack:") !== 0 || row === "pack:auto" || !root.hasSettings(row.slice(5))) return
    root.settingsReturn = row
    root.settingsPath = row.slice(5)
    root.expandedRow = ""
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
    for (var i = 0; i < root.settingsDecl.length; i++) if (root.settingsDecl[i].name === name) return root.settingsDecl[i]
    return null
  }

  // Changes a setting by a step: a toggle flips; a choice moves to the next or previous one (going round when
  // `wrap`); a range moves by its step, within its bounds.
  function stepSetting(name, direction, wrap) {
    var d = root.declOf(name)
    if (!d || !root.svc) return
    var v = root.settingsValues[name]
    var next = v
    if (d.kind === "toggle") next = v ? 0 : 1
    else if (d.kind === "choice") {
      var i = Math.max(0, d.values.indexOf(v)) + direction
      next = d.values[wrap ? (i + d.values.length) % d.values.length : Math.max(0, Math.min(d.values.length - 1, i))]
    } else next = Math.max(d.min, Math.min(d.max, v + direction * d.step))
    if (next !== v) root.svc.setSetting(root.settingsPath, name, next)
  }

  function settingValueText(d) {
    var v = root.settingsValues[d.name]
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
    if (row === "tabs") setTab(root.tab === "about" ? "screensaver" : "about")
    else if (row.indexOf("about:") === 0) openLink(row)
    else if (row === "enable") toggleOnIdle()
    else if (row === "delay") stepDelay(1)
    else if (row === "settings:back") closeSettings()
    else if (row === "settings:live") { if (svc) svc.setLivePreviews(!root.liveOn) }
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
    // Then it shows there from the start, and its row opens after a moment, as when it's moved there.
    cursorActive = true
    pointerSeen = false
    keyMoved = false
    expandedRow = ""
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
    contentWidth: panel.fittedContentWidth(Style.space(380))
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
          root.setTab(dx > 0 ? "about" : "screensaver")
        } else if (dx !== 0 && root.rows[root.cursor].indexOf("set:") === 0) {
          root.stepSetting(root.rows[root.cursor].slice(4), dx > 0 ? 1 : -1, false)
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
          width: header.width
          height: Math.max(tabRow.implicitHeight, enableRow.implicitHeight)

          Row {
            id: tabRow
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.space(8)

            Repeater {
              model: [{ value: "screensaver", label: "Screensaver" }, { value: "about", label: "About" }]
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
            anchors.verticalCenter: parent.verticalCenter
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

      // A pack's settings, in place of the list.
      Flickable {
        id: pageScroller
        visible: root.tab === "screensaver" && root.settingsPath !== ""
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: header.bottom
        anchors.bottom: footer.top
        contentWidth: width
        contentHeight: settingsPage.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        Column {
          id: settingsPage
          width: parent.width
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

          // The pack as its settings now are: live, or a thumbnail drawn again as they change.
          ThumbView {
            id: pageThumbBox
            width: parent.width
            height: Math.round(width / root.screenAspect)
            thumb: root.pageThumb
          }

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

          PanelSeparator { foreground: root.foreground }

          Item { width: 1; height: Style.space(4) }

          Repeater {
            model: root.settingsDecl
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
        // Scrolled by the user (dragged, flicked or wheeled, not a correction of its own as rows resize): the
        // cursor's row is anchored where it now is.
        onContentYChanged: if (!root.adjusting && (moving || dragging || flicking) && root.revealTarget
                               && root.revealTarget.hasCursor) root.anchor(root.revealTarget)
        onMovementEnded: if (root.revealTarget && root.revealTarget.hasCursor) root.anchor(root.revealTarget)
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
        visible: root.tab === "screensaver" && !(root.settingsPath !== "" && root.liveOn)
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
  // can be starred, with its star or the "f" key, to list it first. The row the cursor rests on opens, to show a
  // thumbnail of the pack beside its description; every row opens to the same height.
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

    // Open while the cursor rests on it, and only then: the tick alone marks the chosen pack.
    readonly property bool expanded: root.expandedRow === row
    // How much taller the row is while the cursor holds it open, for the panel's height, which doesn't change with
    // the cursor.
    readonly property real openHeight: expanded ? expansion.implicitHeight + choiceColumn.spacing : 0
    // The thumbnail: the service's "pixels W H <base64 RGB>", "" while it's being drawn, or "none".
    readonly property string thumb: !expanded ? "" : !usable ? "none"
      : root.svc ? root.svc.thumbnailOf(row.slice(5), root.thumbSize) : "none"
    // Each time the row opens, the thumbnail is checked against the pack as it is now.
    function askForThumb() { if (expanded && usable && root.svc) root.svc.requestThumbnail(row.slice(5), root.thumbSize) }
    onThumbChanged: if (thumb === "") askForThumb()

    hasCursor: root.at(row)
    onHasCursorChanged: if (hasCursor) {
      root.anchor(choiceRow)
      root.reveal(choiceRow)
    }
    onExpandedChanged: askForThumb()
    // Once it has grown open, the cursor's row is kept in view.
    onHeightChanged: if (hasCursor) root.reveal(choiceRow)
    // A row made anew where a starred or unstarred pack now sits may already have the cursor.
    Component.onCompleted: if (hasCursor && root.keyMoved) Qt.callLater(function() { root.reveal(choiceRow) })
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
      onClicked: root.choose(choiceRow.row)
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

      // Open: the thumbnail on the left, the description on the right, cut to the thumbnail's height.
      RowLayout {
        id: expansion
        visible: choiceRow.expanded
        Layout.fillWidth: true
        spacing: Style.space(10)
        // Opening a row adds a whole number of rows' height, so the rows below shift by whole rows and the panel's
        // bottom edge still cuts through the middle of one.
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
    readonly property string row: "set:" + decl.name
    readonly property var value: root.settingsValues[decl.name]
    hasCursor: root.at(row)
    foreground: root.foreground
    implicitHeight: settingLayout.implicitHeight + Style.spacing.rowPaddingX
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
              onClicked: if (root.svc) root.svc.setSetting(root.settingsPath, settingRow.decl.name, settingRow.decl.values[index])
            }
          }
        }
      }

      // A range: its value between steps down and up.
      RowLayout {
        visible: settingRow.decl.kind === "range"
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
