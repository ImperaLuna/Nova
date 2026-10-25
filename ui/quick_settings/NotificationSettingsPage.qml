pragma ComponentBehavior: Bound

import QtQuick
import "../launcher"
import "../notifications"
import "../theme"

// The notifications' settings page: do not disturb, and a switch per app
// that has sent one (off mutes it: listed, but no pop-ups). Properties in,
// signals out, like the other pages.
Item {
    id: page

    property bool doNotDisturb: false
    // A notifications event's apps ({name, icon, app_id}) and muted names.
    property var apps: []
    property var muted: []
    property var iconSource: name => ""
    // Room the top row leaves on its right (the pill's buttons).
    property real headerInset: 0

    signal back
    signal doNotDisturbToggled
    signal muteToggled(string app, bool muted)

    Column {
        id: settings
        width: parent.width
        spacing: Tokens.quickSettingsGap

        PageHeader {
            width: parent.width - page.headerInset
            title: qsTr("Notifications")
            onBack: page.back()
        }

        SwitchRow {
            width: parent.width
            title: qsTr("Do not disturb")
            subtitle: qsTr("Hide pop-ups, keep them in the list")
            on: page.doNotDisturb
            selected: true
            onClicked: page.doNotDisturbToggled()
        }

        CardTitle {
            x: Tokens.pad
            visible: page.apps.length > 0
            text: qsTr("Apps")
        }
    }

    Flickable {
        anchors.top: settings.bottom
        anchors.topMargin: Tokens.gap
        anchors.bottom: parent.bottom
        width: parent.width
        contentHeight: appList.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        Column {
            id: appList
            width: parent.width

            Repeater {
                model: page.apps

                SwitchRow {
                    id: appRow
                    required property var modelData
                    readonly property bool isMuted: page.muted.includes(modelData.name)
                    width: appList.width
                    title: modelData.name
                    subtitle: isMuted ? qsTr("Muted: list only, no pop-ups") : qsTr("Pop-ups and list")
                    on: !isMuted
                    onClicked: page.muteToggled(modelData.name, !isMuted)

                    AppIcon {
                        parent: appRow.lead
                        anchors.centerIn: parent
                        size: Tokens.notificationRowIcon
                        appId: appRow.modelData.app_id ?? appRow.modelData.name
                        name: appRow.modelData.name
                        source: page.iconSource(appRow.modelData.icon ?? "")
                    }
                }
            }
        }
    }

    // A row with a name, a line under it and a switch at its end; `lead`
    // holds an icon before the name, when there is one.
    component SwitchRow: Item {
        id: row
        required property string title
        property string subtitle: ""
        property bool on: false
        property bool selected: false
        readonly property Item lead: leadSlot
        signal clicked

        implicitHeight: Tokens.powerRowHeight

        Rectangle {
            anchors.fill: parent
            radius: Tokens.radiusRow
            color: press.containsMouse ? Tokens.hover : row.selected ? Tokens.surfaceRaised : "transparent"
        }
        MouseArea {
            id: press
            anchors.fill: parent
            hoverEnabled: true
            onClicked: row.clicked()
        }
        Item {
            id: leadSlot
            x: Tokens.pad
            width: children.length > 0 ? Tokens.notificationRowIcon : 0
            height: parent.height
        }
        Column {
            anchors.verticalCenter: parent.verticalCenter
            x: leadSlot.x + leadSlot.width + (leadSlot.width > 0 ? Tokens.pad : 0)
            width: toggle.x - x - Tokens.gap

            Text {
                width: parent.width
                elide: Text.ElideRight
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                font.weight: Font.Medium
                color: Tokens.fg
                textFormat: Text.PlainText
                text: row.title
            }
            Text {
                visible: text !== ""
                width: parent.width
                elide: Text.ElideRight
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeTiny
                color: Tokens.fgDim
                textFormat: Text.PlainText
                text: row.subtitle
            }
        }
        ToggleSwitch {
            id: toggle
            anchors.right: parent.right
            anchors.rightMargin: Tokens.pad
            anchors.verticalCenter: parent.verticalCenter
            on: row.on
        }
    }
}
