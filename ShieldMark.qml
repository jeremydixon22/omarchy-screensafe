import QtQuick
import QtQuick.Shapes
import qs.Commons

// ScreenSafe's mark: a shield with three points along its top, coming to a point at the bottom, with a section
// sign (§) cut through it. It takes the theme's accent, its left half a shade lighter, and the § is a hole, so
// whatever is behind the mark shows through it.
Item {
  id: root

  property color accent: Color.accent
  property string fontFamily: Style.font.family

  implicitWidth: 16
  implicitHeight: 16

  // The shield is 14 wide and 16 tall, centered in the item.
  readonly property real unit: Math.min(width / 14, height / 16)
  readonly property real originX: (width - 14 * unit) / 2
  readonly property real originY: (height - 16 * unit) / 2
  function px(u) { return originX + u * 14 * unit }
  function py(v) { return originY + v * 16 * unit }

  readonly property font glyphFont: Qt.font({ family: root.fontFamily, bold: true, pixelSize: Math.max(1, Math.round(10 * root.unit)) })

  TextMetrics {
    id: glyph
    font: root.glyphFont
    text: "§"
  }

  // PathText starts the glyph at x, its ink beginning the glyph's left bearing further on; its outline's top is at
  // y. The § is centered across the shield by its ink, and sits a little below the shield's middle, where it looks
  // centered between the points along the top and the point below.
  readonly property real glyphX: px(0.5) - glyph.tightBoundingRect.width / 2 - glyph.tightBoundingRect.x
  readonly property real glyphY: py(0.54) - glyph.tightBoundingRect.height / 2

  component Shield: Shape {
    id: shield
    property color fill
    antialiasing: true
    preferredRendererType: Shape.CurveRenderer

    ShapePath {
      fillColor: shield.fill
      fillRule: ShapePath.OddEvenFill
      strokeWidth: -1
      startX: root.px(0); startY: root.py(0.02)
      PathLine { x: root.px(0.25); y: root.py(0.15) }
      PathLine { x: root.px(0.5); y: root.py(0) }
      PathLine { x: root.px(0.75); y: root.py(0.15) }
      PathLine { x: root.px(1); y: root.py(0.02) }
      PathLine { x: root.px(1); y: root.py(0.46) }
      PathQuad { x: root.px(0.5); y: root.py(1); controlX: root.px(1); controlY: root.py(0.8) }
      PathQuad { x: root.px(0); y: root.py(0.46); controlX: root.px(0); controlY: root.py(0.8) }
      PathLine { x: root.px(0); y: root.py(0.02) }
      // Inside the shield, so the odd-even rule leaves it empty.
      PathText { x: root.glyphX; y: root.glyphY; font: root.glyphFont; text: "§" }
    }
  }

  // The whole shield in the accent.
  Shield {
    anchors.fill: parent
    fill: root.accent
  }

  // The same shield, lighter, showing only its left half.
  Item {
    x: 0
    y: 0
    width: root.px(0.5)
    height: root.height
    clip: true

    Shield {
      width: root.width
      height: root.height
      fill: Qt.lighter(root.accent, 1.2)
    }
  }
}
