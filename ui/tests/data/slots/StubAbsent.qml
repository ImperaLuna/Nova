import QtQuick
import "../../../quick_settings"
import "../../../theme"

// A slot view with nothing to show now, for MainPage's layout tests.
QsSlot {
    objectName: "stubAbsent"
    present: false
    implicitHeight: Tokens.tileHeight
}
