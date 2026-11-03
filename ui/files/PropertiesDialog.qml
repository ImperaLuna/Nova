pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "properties.js" as Props

// The Properties dialog in the middle of a Nova Files window (Alt+Enter,
// a drive's menu), after Explorer's General page: the item's big icon and
// name, then its facts in groups with a rule between (type, opens with,
// location; size, size on disk, contains; created, modified, accessed),
// a drive's fill bar, and for one item who may read, change and run it,
// which its owner can tick. "Change" beside "Opens with" lists the other
// apps; picking one makes it the default. The size counts up as novad
// measures. Close (the default), Enter and Escape close it.
// It only shows what it's given; PropertiesModel asks novad.
Item {
    id: dialog

    // Props.subject(...).
    required property var subject
    // novad's files_properties answer for the one item, or null.
    property var properties: null
    // The size measured so far (Props.total), or null.
    property var counted: null
    // The apps that open the one file ({id, name}), the default first, and
    // the default's id ("" for none).
    property var apps: []
    property string defaultApp: ""

    signal closeRequested
    // Who (owner, group, others) should or shouldn't have a right (read,
    // write, run) any more.
    signal accessFlipped(string who, string right)
    signal defaultChosen(string app)

    property bool choosingApp: false
    readonly property var openWith: defaultApp !== "" && apps.length > 0 ? apps[0] : null
    readonly property var groups: Props.groups(subject, properties, counted, openWith)
    readonly property bool single: !subject.drive && subject.rows.length <= 1 && properties !== null
    readonly property var drive: subject.drive ? properties?.drive ?? null : null

    implicitWidth: Tokens.filesPropertiesWidth
    implicitHeight: content.implicitHeight + Tokens.panelPad + Tokens.panelPadBottom

    onSubjectChanged: choosingApp = false

    Keys.onReturnPressed: closeRequested()
    Keys.onEnterPressed: closeRequested()
    Keys.onEscapePressed: closeRequested()

    GlassShadow {
        glass: surface
    }
    Glass {
        id: surface
        anchors.fill: parent
        radius: Tokens.radiusPanel
    }
    // Holds clicks, so one on the dialog doesn't close it.
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.AllButtons
    }

    component Rule: Rectangle {
        width: content.width
        height: Tokens.dividerWidth
        color: Tokens.filesRule
    }

    component Label: Text {
        width: Tokens.filesPropertiesLabelWidth
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSize
        color: Tokens.fgDim
        elide: Text.ElideRight
    }

    Column {
        id: content
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Tokens.panelPad
        spacing: Tokens.pad

        Row {
            spacing: Tokens.pad

            LineIcon {
                anchors.verticalCenter: parent.verticalCenter
                path: Props.icon(dialog.subject, dialog.properties)
                size: Tokens.filesClashIconSize
                color: (dialog.properties?.entry?.dir ?? false) && !dialog.subject.drive ? Tokens.filesFolder : Tokens.fgDim
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                width: content.width - Tokens.filesClashIconSize - Tokens.pad
                text: dialog.subject.title
                textFormat: Text.PlainText
                elide: Text.ElideMiddle
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeTitle
                font.weight: Tokens.weightTitle
                color: Tokens.fg
            }
        }

        Repeater {
            model: dialog.groups

            Column {
                id: group

                required property var modelData

                width: content.width
                spacing: Tokens.pad

                Rule {}
                Column {
                    width: parent.width

                    Repeater {
                        model: group.modelData

                        Column {
                            id: fact

                            required property var modelData

                            width: content.width

                            Item {
                                width: content.width
                                height: Math.max(Tokens.filesPropertiesRowHeight, value.implicitHeight)

                                Label {
                                    anchors.top: parent.top
                                    height: Tokens.filesPropertiesRowHeight
                                    verticalAlignment: Text.AlignVCenter
                                    text: fact.modelData.label
                                }
                                Text {
                                    id: value
                                    x: Tokens.filesPropertiesLabelWidth
                                    width: parent.width - x - (change.visible ? change.width + Tokens.gap : 0)
                                    topPadding: (Tokens.filesPropertiesRowHeight - fontMetrics.height) / 2
                                    text: fact.modelData.value
                                    textFormat: Text.PlainText
                                    wrapMode: Text.WrapAnywhere
                                    maximumLineCount: 3
                                    elide: Text.ElideRight
                                    font.family: Tokens.fontSans
                                    font.pixelSize: Tokens.fontSize
                                    color: Tokens.fg

                                    FontMetrics {
                                        id: fontMetrics
                                        font: value.font
                                    }
                                }
                                DialogButton {
                                    id: change
                                    anchors.right: parent.right
                                    anchors.verticalCenter: parent.verticalCenter
                                    visible: fact.modelData.change === true && dialog.apps.length > 1
                                    height: Tokens.filesPropertiesRowHeight
                                    quiet: true
                                    text: qsTr("Change")
                                    onClicked: dialog.choosingApp = !dialog.choosingApp
                                }
                            }
                            // The other apps, right under "Opens with" while choosing.
                            Repeater {
                                model: dialog.choosingApp && fact.modelData.change === true ? dialog.apps.slice(1) : []

                                Rectangle {
                                    id: app

                                    required property var modelData

                                    width: content.width
                                    height: Tokens.menuRowHeight
                                    radius: Tokens.menuRowRadius
                                    color: appHover.hovered ? Tokens.selected : "transparent"

                                    Text {
                                        anchors.verticalCenter: parent.verticalCenter
                                        x: Tokens.filesPropertiesLabelWidth
                                        width: parent.width - x - Tokens.rowInset
                                        text: app.modelData.name
                                        textFormat: Text.PlainText
                                        elide: Text.ElideRight
                                        font.family: Tokens.fontSans
                                        font.pixelSize: Tokens.fontSize
                                        color: Tokens.fgDim
                                    }
                                    HoverHandler {
                                        id: appHover
                                    }
                                    TapHandler {
                                        onTapped: {
                                            dialog.choosingApp = false;
                                            dialog.defaultChosen(app.modelData.id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // How full a drive is.
        Rectangle {
            visible: dialog.drive !== null
            width: content.width
            height: Tokens.filesPropertiesBarHeight
            radius: height / 2
            color: Tokens.track

            Rectangle {
                width: parent.width * Props.fill(dialog.drive)
                height: parent.height
                radius: parent.radius
                color: Tokens.meter
            }
        }

        // Who may read, change and run it.
        Column {
            id: access
            visible: dialog.single
            width: content.width
            spacing: Tokens.pad

            readonly property var columns: Props.rightsColumns(dialog.properties?.entry?.dir ?? false)

            Rule {}
            Row {
                x: content.width - access.columns.length * Tokens.filesPropertiesCheckColumn

                Repeater {
                    model: access.columns

                    Text {
                        required property var modelData

                        width: Tokens.filesPropertiesCheckColumn
                        horizontalAlignment: Text.AlignHCenter
                        text: modelData.label
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSizeSmall
                        font.weight: Tokens.weightLabel
                        color: Tokens.filesFaint
                    }
                }
            }
            Column {
                width: content.width

                Repeater {
                    model: Props.people(dialog.properties)

                    Item {
                        id: person

                        required property var modelData

                        width: content.width
                        height: Tokens.filesPropertiesRowHeight

                        Label {
                            anchors.verticalCenter: parent.verticalCenter
                            text: person.modelData.who
                        }
                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            x: Tokens.filesPropertiesLabelWidth
                            width: parent.width - x - access.columns.length * Tokens.filesPropertiesCheckColumn
                            text: person.modelData.name
                            textFormat: Text.PlainText
                            elide: Text.ElideRight
                            font.family: Tokens.fontSans
                            font.pixelSize: Tokens.fontSize
                            color: Tokens.fg
                        }
                        Row {
                            anchors.right: parent.right
                            height: parent.height

                            Repeater {
                                model: access.columns

                                Item {
                                    id: rightCell

                                    required property var modelData

                                    width: Tokens.filesPropertiesCheckColumn
                                    height: parent.height

                                    CheckMark {
                                        anchors.centerIn: parent
                                        enabled: dialog.properties?.yours ?? false
                                        checked: person.modelData.rights[rightCell.modelData.key]
                                    }
                                    TapHandler {
                                        enabled: dialog.properties?.yours ?? false
                                        onTapped: dialog.accessFlipped(person.modelData.key, rightCell.modelData.key)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        DialogButton {
            anchors.right: parent.right
            text: qsTr("Close")
            accent: true
            minWidth: Tokens.filesDialogButtonMinWidth
            onClicked: dialog.closeRequested()
        }
    }
}
