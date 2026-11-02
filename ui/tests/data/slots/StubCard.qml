import QtQuick
import "../../../quick_settings"
import "../../../theme"

// A card-sized slot view for MainPage's layout tests; a click asks for a
// page.
QsSlot {
    id: slot
    objectName: "stubCard"
    implicitHeight: 2 * Tokens.tileHeight

    MouseArea {
        anchors.fill: parent
        onClicked: slot.qs.pageRequested("stub.page")
    }
}
