import QtQuick
import QtQuick.Layouts
import qs.Commons
import qs.Ui
import "ColorModel.js" as ColorModel

// Shared, bounded theme/custom picker. All editing is pointer/keyboard geometry;
// the hex label is read-only, so no native clipboard/text-input path is introduced.
Item {
  id: picker
  property real value: 0
  property var swatches: []
  property color foreground: Color.foreground
  property string fontFamily: Style.font.family
  property string cursorChannel: ""
  property bool selected: false
  property bool compact: false
  property real previewValue: value
  property real hue: 0
  property real saturation: 1
  property real brightness: 1
  property bool ready: false
  readonly property bool customMode: previewValue >= ColorModel.customBase
  readonly property bool interacting: svMouse.pressed || hueSlider.dragging || satSlider.dragging || valueSlider.dragging
  signal picked(real selection)
  signal pointing(var area, var mouse, string channel)
  signal channelActivated(string channel)
  signal revealRequested(var item)
  implicitHeight: body.implicitHeight
  function commit(selection) { if (ColorModel.valid(selection) && selection !== value) picker.picked(selection) }
  function sync() {
    var c = ColorModel.hsv(ColorModel.hex(previewValue, swatches))
    if (c.s > 0 && c.v > 0) hue = c.h
    saturation = c.s; brightness = c.v
  }
  onValueChanged: if (!interacting) { previewValue = value; sync() }
  onSwatchesChanged: if (!interacting) sync()
  Component.onCompleted: { sync(); ready = true }
  function edit(selection, final) {
    previewValue = selection
    if (final) { pending.stop(); picker.commit(selection) }
    else if (!pending.running) pending.start()
  }
  function editHsv(final) { edit(ColorModel.fromHsv(hue, saturation, brightness), final) }
  function choose(selection) { pending.stop(); picker.channelActivated(""); previewValue = selection; sync(); picker.commit(selection) }
  // Limit persistent writes/native preview reloads during dragging; always flush release.
  Timer { id: pending; interval: 150; onTriggered: picker.commit(picker.previewValue) }
  // A saved custom color must not scroll a newly opened page past its live preview.
  onCustomModeChanged: if (customMode && ready && selected) customReveal.restart()
  Timer { id: customReveal; interval: 1; onTriggered: if (picker.customMode) picker.revealRequested(sv) }
  Column {
    id: body
    width: parent.width
    spacing: Style.space(picker.compact ? 4 : 6)
    Flow {
      width: parent.width
      spacing: Style.space(picker.compact ? 3 : 4)
      Repeater {
        model: [{ value: 0, name: "Theme colors", color: ColorModel.hex(0, picker.swatches) }].concat(picker.swatches)
        Rectangle {
          id: swatch
          required property var modelData
          width: Style.space(23); height: width
          color: modelData.color
          border.color: picker.previewValue === modelData.value ? picker.foreground : "transparent"
          border.width: picker.previewValue === modelData.value ? Style.space(2) : 0
          Text {
            anchors.centerIn: parent
            text: swatch.modelData.value === 0 ? "T" : picker.previewValue === swatch.modelData.value ? "✓" : ""
            color: ColorModel.textColor(swatch.modelData.color)
            font.family: picker.fontFamily
            font.pixelSize: Style.font.caption
            font.bold: true
          }
          MouseArea {
            id: swatchMouse
            anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
            onPositionChanged: function(mouse) { picker.pointing(swatchMouse, mouse, "") }
            onClicked: picker.choose(swatch.modelData.value)
          }
          PanelToolTip { visible: swatchMouse.containsMouse; text: swatch.modelData.name.replace(/_/g, " "); fontFamily: picker.fontFamily }
        }
      }
    }
    RowLayout {
      width: parent.width
      Button { text: "Custom"; selected: picker.customMode; bordered: true; foreground: picker.foreground; fontFamily: picker.fontFamily; onClicked: picker.choose(ColorModel.custom(ColorModel.hex(picker.previewValue, picker.swatches))) }
      Text { Layout.fillWidth: true; text: ColorModel.name(picker.previewValue, picker.swatches); textFormat: Text.PlainText; color: picker.foreground; font.family: picker.fontFamily; font.pixelSize: Style.font.caption; elide: Text.ElideRight; horizontalAlignment: Text.AlignRight }
    }
    Item {
      id: customControls
      visible: picker.customMode
      width: parent.width
      implicitHeight: picker.compact ? Math.max(sv.height, channels.implicitHeight) : sv.height + Style.space(6) + channels.implicitHeight
      Rectangle {
        id: sv
        visible: picker.customMode
        width: picker.compact ? Math.min(Style.space(124), parent.width * .34) : parent.width; height: Style.space(88)
        color: Qt.hsva(picker.hue, 1, 1, 1)
        Rectangle { anchors.fill: parent; gradient: Gradient { orientation: Gradient.Horizontal; GradientStop { position: 0; color: "white" } GradientStop { position: 1; color: "transparent" } } }
        Rectangle { anchors.fill: parent; gradient: Gradient { GradientStop { position: 0; color: "transparent" } GradientStop { position: 1; color: "black" } } }
        Rectangle { width: Style.space(9); height: width; radius: width/2; color: "transparent"; border.width: 2; border.color: "white"; x: Math.max(0, Math.min(sv.width-width, picker.saturation * sv.width - width/2)); y: Math.max(0, Math.min(sv.height-height, (1-picker.brightness) * sv.height - height/2)) }
        MouseArea {
          id: svMouse
          anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.CrossCursor
          function update(mouse, final) { picker.saturation = Math.max(0, Math.min(1, mouse.x / width)); picker.brightness = 1 - Math.max(0, Math.min(1, mouse.y / height)); picker.editHsv(final) }
          onPressed: function(mouse) { picker.channelActivated("s"); update(mouse, false) }
          onPositionChanged: function(mouse) { picker.pointing(svMouse, mouse, "s"); if (pressed) update(mouse, false) }
          onReleased: function(mouse) { update(mouse, true) }
          onCanceled: picker.editHsv(true)
        }
      }
      Column {
        id: channels
        x: picker.compact ? sv.width + Style.space(8) : 0
        y: picker.compact ? 0 : sv.height + Style.space(6)
        width: parent.width - x
        spacing: Style.space(picker.compact ? 4 : 6)
        // Explicit focus rows let the panel's existing Up/Down and Left/Right model edit HSV too.
        CursorSurface {
          id: hueRow; visible: picker.customMode; width: parent.width; implicitHeight: Style.space(26); foreground: picker.foreground; hasCursor: picker.cursorChannel === "h"
          onHasCursorChanged: if (hasCursor) picker.revealRequested(hueRow)
          RowLayout {
            anchors.fill: parent; spacing: Style.space(8)
            Text { text: "Hue"; Layout.preferredWidth: Style.space(40); color: picker.foreground; font.family: picker.fontFamily; font.pixelSize: Style.font.caption }
            Item {
              Layout.fillWidth: true; Layout.fillHeight: true
              Rectangle { anchors.left: parent.left; anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter; height: Style.space(6); gradient: Gradient { orientation: Gradient.Horizontal; GradientStop { position: 0; color: "#ff0000" } GradientStop { position: .167; color: "#ffff00" } GradientStop { position: .333; color: "#00ff00" } GradientStop { position: .5; color: "#00ffff" } GradientStop { position: .667; color: "#0000ff" } GradientStop { position: .833; color: "#ff00ff" } GradientStop { position: 1; color: "#ff0000" } } }
              PanelSlider { id: hueSlider; anchors.fill: parent; value: picker.hue; minimum: 0; maximum: 1; trackColor: "transparent"; fillColor: "transparent"; knobColor: picker.foreground; onMoved: function(v) { picker.channelActivated("h"); picker.hue = v; picker.editHsv(false) }; onReleased: function(v) { picker.hue = v; picker.editHsv(true) } }
            }
            Text { text: Math.round(picker.hue*360) + "°"; Layout.preferredWidth: Style.space(35); color: picker.foreground; font.family: picker.fontFamily; font.pixelSize: Style.font.caption; horizontalAlignment: Text.AlignRight }
          }
        }
        CursorSurface {
          id: satRow; visible: picker.customMode; width: parent.width; implicitHeight: Style.space(26); foreground: picker.foreground; hasCursor: picker.cursorChannel === "s"
          onHasCursorChanged: if (hasCursor) picker.revealRequested(satRow)
          RowLayout {
            anchors.fill: parent; spacing: Style.space(8)
            Text { text: "Sat."; Layout.preferredWidth: Style.space(40); color: picker.foreground; font.family: picker.fontFamily; font.pixelSize: Style.font.caption }
            PanelSlider { id: satSlider; Layout.fillWidth: true; value: picker.saturation; trackColor: Qt.darker(picker.foreground, 2.5); fillColor: picker.foreground; knobColor: picker.foreground; onMoved: function(v) { picker.channelActivated("s"); picker.saturation = v; picker.editHsv(false) }; onReleased: function(v) { picker.saturation = v; picker.editHsv(true) } }
            Text { text: Math.round(picker.saturation*100) + "%"; Layout.preferredWidth: Style.space(35); color: picker.foreground; font.family: picker.fontFamily; font.pixelSize: Style.font.caption; horizontalAlignment: Text.AlignRight }
          }
        }
        CursorSurface {
          id: valueRow; visible: picker.customMode; width: parent.width; implicitHeight: Style.space(26); foreground: picker.foreground; hasCursor: picker.cursorChannel === "v"
          onHasCursorChanged: if (hasCursor) picker.revealRequested(valueRow)
          RowLayout {
            anchors.fill: parent; spacing: Style.space(8)
            Text { text: "Value"; Layout.preferredWidth: Style.space(40); color: picker.foreground; font.family: picker.fontFamily; font.pixelSize: Style.font.caption }
            PanelSlider { id: valueSlider; Layout.fillWidth: true; value: picker.brightness; trackColor: Qt.darker(picker.foreground, 2.5); fillColor: picker.foreground; knobColor: picker.foreground; onMoved: function(v) { picker.channelActivated("v"); picker.brightness = v; picker.editHsv(false) }; onReleased: function(v) { picker.brightness = v; picker.editHsv(true) } }
            Text { text: Math.round(picker.brightness*100) + "%"; Layout.preferredWidth: Style.space(35); color: picker.foreground; font.family: picker.fontFamily; font.pixelSize: Style.font.caption; horizontalAlignment: Text.AlignRight }
          }
        }
      }
    }
    Text { visible: picker.customMode && !picker.compact; width: parent.width; text: "Brightness adapts to preserve highlights and shadows."; wrapMode: Text.Wrap; color: Qt.darker(picker.foreground, 1.4); font.family: picker.fontFamily; font.pixelSize: Style.font.caption; textFormat: Text.PlainText }
  }
}
