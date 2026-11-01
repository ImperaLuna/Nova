pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "quick_settings.js" as QuickSettings

// The resources page: CPU and memory over the last minutes, and the
// apps using the most memory now (each with all its processes), from
// novad's resources events. An app's row can end it.
Item {
    id: page

    // A resources event's data, or null before the first.
    property var reading: null
    signal back
    // Room the top row leaves on its right (the pill's buttons).
    property real headerInset: 0
    // End, confirmed on an app's row: novad's end_app for its key.
    signal endRequested(string key)

    readonly property var apps: reading?.apps ?? []
    // What the search field holds, and the apps whose name has it.
    property string query: ""
    readonly property var shownApps: query === "" ? apps : apps.filter(app => app.name.toLowerCase().includes(query.toLowerCase()))
    // The app whose End asks to confirm, by key.
    property string asked: ""
    onQueryChanged: asked = ""

    readonly property real memoryShare: reading ? QuickSettings.share(reading.memory_used, reading.memory_total) * 100 : 0

    component Legend: Row {
        id: legend
        required property color color
        required property string text
        spacing: Tokens.iconGap

        Rectangle {
            anchors.verticalCenter: parent.verticalCenter
            width: Tokens.legendDot
            height: width
            radius: width / 2
            color: legend.color
        }
        CardTitle {
            text: legend.text
        }
    }

    component Cell: Text {
        font.family: Tokens.fontMono
        font.pixelSize: Tokens.fontSizeSmall
        horizontalAlignment: Text.AlignRight
        width: Tokens.processNumberWidth
    }

    Column {
        width: page.width
        spacing: Tokens.quickSettingsGap

        PageHeader {
            width: parent.width - page.headerInset
            title: qsTr("Resources")
            onBack: page.back()
        }

        Rectangle {
            width: parent.width
            height: graphColumn.implicitHeight + 2 * Tokens.pad
            radius: Tokens.radiusCard
            color: Tokens.surfaceRaised

            Column {
                id: graphColumn
                x: Tokens.pad
                y: Tokens.pad
                width: parent.width - 2 * Tokens.pad
                spacing: Tokens.gap

                Row {
                    spacing: Tokens.pad
                    Legend {
                        color: Tokens.primary
                        text: qsTr("CPU %1").arg(page.reading ? QuickSettings.percent(page.reading.cpu) : "—")
                    }
                    Legend {
                        color: Tokens.secondary
                        text: qsTr("RAM %1").arg(page.reading ? QuickSettings.percent(page.memoryShare) : "—")
                    }
                }

                Graph {
                    width: parent.width
                    history: page.reading?.history ?? []
                    span: page.reading?.history_size ?? 1
                }

                Item {
                    width: parent.width
                    height: past.implicitHeight
                    Text {
                        id: past
                        font.family: Tokens.fontMono
                        font.pixelSize: Tokens.fontSizeMicro
                        color: Tokens.fgDim
                        text: qsTr("−3 min")
                    }
                    Text {
                        anchors.right: parent.right
                        font.family: Tokens.fontMono
                        font.pixelSize: Tokens.fontSizeMicro
                        color: Tokens.fgDim
                        text: qsTr("now")
                    }
                }
            }
        }

        Column {
            id: appsBox
            width: parent.width

            // Searches the apps listed by name.
            Rectangle {
                width: parent.width
                height: Tokens.searchHeight
                radius: height / 2
                color: Tokens.track

                LineIcon {
                    id: searchIcon
                    x: Tokens.pad
                    anchors.verticalCenter: parent.verticalCenter
                    path: Icons.SEARCH
                    size: Tokens.iconSizeSmall
                    color: Tokens.fgDim
                }
                TextInput {
                    id: search
                    objectName: "search"
                    anchors.verticalCenter: parent.verticalCenter
                    x: searchIcon.x + searchIcon.width + Tokens.gap
                    width: parent.width - x - Tokens.pad
                    clip: true
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeSmall
                    color: Tokens.fg
                    selectionColor: Tokens.secondary
                    selectedTextColor: Tokens.onAccent
                    onTextChanged: page.query = text.trim()
                    Keys.onEscapePressed: event => {
                        // Clears the search first; with none, Esc closes the drawer.
                        event.accepted = text !== "";
                        text = "";
                    }

                    Text {
                        visible: search.text === ""
                        font: search.font
                        color: Tokens.fgDim
                        text: qsTr("Search apps")
                    }
                }
            }

            Item {
                width: parent.width
                height: Tokens.chipHeight + Tokens.gap
                Text {
                    x: Tokens.pad
                    anchors.bottom: parent.bottom
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeSmall
                    color: Tokens.fgDim
                    text: qsTr("Apps by memory")
                }
                Row {
                    anchors.right: parent.right
                    anchors.rightMargin: Tokens.segmentPad + Tokens.endButtonWidth
                    anchors.bottom: parent.bottom
                    Cell {
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSizeSmall
                        color: Tokens.fgDim
                        text: qsTr("CPU")
                    }
                    Cell {
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSizeSmall
                        color: Tokens.fgDim
                        text: qsTr("Memory")
                    }
                }
            }

            // The rest of the page, scrolling when there are more apps.
            Flickable {
                id: list
                objectName: "apps"
                width: parent.width
                height: Math.max(0, page.height - appsBox.y - y)
                contentHeight: rows.height
                clip: true
                boundsBehavior: Flickable.StopAtBounds

                Column {
                    id: rows
                    width: list.width

                    Text {
                        visible: page.shownApps.length === 0 && page.query !== ""
                        leftPadding: Tokens.pad
                        topPadding: Tokens.gap
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSizeSmall
                        color: Tokens.fgDim
                        textFormat: Text.PlainText
                        text: qsTr("No app matches “%1”").arg(page.query)
                    }

                    // By count, so a new reading every few seconds doesn't
                    // rebuild the rows (and drop a hover or an open confirm).
                    Repeater {
                        model: page.shownApps.length

                        Column {
                            id: entry
                            required property int index
                            readonly property var app: page.shownApps[index] ?? {
                                name: "",
                                key: "",
                                processes: 0,
                                cpu: 0,
                                memory: 0,
                                protected: true
                            }
                            readonly property bool asked: page.asked !== "" && page.asked === app.key
                            width: rows.width

                            Rectangle {
                                id: row
                                objectName: `app-${entry.app.key}`
                                width: parent.width
                                height: Tokens.processRowHeight
                                radius: Tokens.radiusSegment
                                color: hover.containsMouse || entry.asked ? Tokens.surfaceRaised : "transparent"

                                MouseArea {
                                    id: hover
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    acceptedButtons: Qt.NoButton
                                }

                                Row {
                                    x: Tokens.pad
                                    anchors.verticalCenter: parent.verticalCenter
                                    width: numbers.x - x - Tokens.gap
                                    spacing: Tokens.iconGap
                                    Text {
                                        id: appName
                                        width: Math.min(implicitWidth, parent.width - count.width - parent.spacing)
                                        elide: Text.ElideRight
                                        font.family: Tokens.fontSans
                                        font.pixelSize: Tokens.fontSizeSmall
                                        color: Tokens.fg
                                        textFormat: Text.PlainText
                                        text: entry.app.name
                                    }
                                    Text {
                                        id: count
                                        visible: entry.app.processes > 1
                                        anchors.baseline: appName.baseline
                                        font.family: Tokens.fontMono
                                        font.pixelSize: Tokens.fontSizeMicro
                                        color: Tokens.fgDim
                                        text: `×${entry.app.processes}`
                                    }
                                }
                                Row {
                                    id: numbers
                                    anchors.right: parent.right
                                    anchors.rightMargin: Tokens.segmentPad
                                    anchors.verticalCenter: parent.verticalCenter
                                    Cell {
                                        anchors.verticalCenter: parent.verticalCenter
                                        color: Tokens.fg
                                        text: `${entry.app.cpu.toFixed(1)}%`
                                    }
                                    Cell {
                                        anchors.verticalCenter: parent.verticalCenter
                                        color: Tokens.fgDim
                                        text: QuickSettings.bytes(entry.app.memory)
                                    }
                                    // ✕ while hovered: asks to confirm under
                                    // the row. Nova and the session have none.
                                    Item {
                                        width: Tokens.endButtonWidth
                                        height: Tokens.endButtonHeight
                                        Rectangle {
                                            objectName: "end"
                                            anchors.fill: parent
                                            radius: Tokens.radiusSegment
                                            visible: !entry.app.protected && (hover.containsMouse || end.containsMouse || entry.asked)
                                            color: end.containsMouse || entry.asked ? Tokens.alpha(Tokens.secondary, Tokens.endHoverOpacity) : "transparent"

                                            LineIcon {
                                                anchors.centerIn: parent
                                                path: Icons.CLOSE
                                                size: Tokens.endIconSize
                                                color: end.containsMouse || entry.asked ? Tokens.secondary : Tokens.fgDim
                                            }
                                            MouseArea {
                                                id: end
                                                anchors.fill: parent
                                                hoverEnabled: true
                                                onClicked: page.asked = entry.asked ? "" : entry.app.key
                                            }
                                        }
                                    }
                                }
                            }

                            // Asks before ending: the app is asked to close.
                            Rectangle {
                                objectName: `confirm-${entry.app.key}`
                                visible: entry.asked
                                width: parent.width
                                height: visible ? Math.max(question.implicitHeight, Tokens.confirmButtonHeight) + 2 * Tokens.pad : 0
                                radius: Tokens.radiusSegment
                                color: Tokens.alpha(Tokens.secondary, Tokens.confirmOpacity)

                                Text {
                                    id: question
                                    x: Tokens.pad
                                    anchors.verticalCenter: parent.verticalCenter
                                    width: buttons.x - x - Tokens.gap
                                    wrapMode: Text.Wrap
                                    font.family: Tokens.fontSans
                                    font.pixelSize: Tokens.fontSizeSmall
                                    color: Tokens.fg
                                    textFormat: Text.PlainText
                                    text: qsTr("End %1?").arg(entry.app.name)
                                }
                                Row {
                                    id: buttons
                                    anchors.right: parent.right
                                    anchors.rightMargin: Tokens.pad
                                    anchors.verticalCenter: parent.verticalCenter
                                    spacing: Tokens.iconGap
                                    ConfirmButton {
                                        text: qsTr("Cancel")
                                        onClicked: page.asked = ""
                                    }
                                    ConfirmButton {
                                        objectName: "endConfirm"
                                        text: qsTr("End")
                                        accent: true
                                        onClicked: {
                                            page.asked = "";
                                            page.endRequested(entry.app.key);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    component ConfirmButton: Rectangle {
        id: confirmButton
        property alias text: label.text
        property bool accent: false
        signal clicked
        width: label.implicitWidth + 2 * Tokens.pad
        height: Tokens.confirmButtonHeight
        radius: Tokens.radiusSegment
        color: accent ? Tokens.secondary : press.containsMouse ? Tokens.hover : Tokens.track

        Text {
            id: label
            anchors.centerIn: parent
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeSmall
            font.weight: confirmButton.accent ? Tokens.weightTitle : Tokens.weightBody
            color: confirmButton.accent ? Tokens.onAccent : Tokens.fg
        }
        MouseArea {
            id: press
            anchors.fill: parent
            hoverEnabled: true
            onClicked: confirmButton.clicked()
        }
    }
}
