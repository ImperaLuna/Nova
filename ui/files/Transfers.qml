pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "service"
import "../daemon/protocol.js" as Protocol
import "paths.js" as Paths
import "transfers.js" as Transfers

// A Nova Files window's copies and moves, over everything else in it: the
// jobs it started, each as a card stacked at the bottom right, and the name
// clash dialog in the middle while one of them asks. Clashes come one at a
// time, oldest job first; the window behind takes no clicks meanwhile.
Item {
    id: transfers

    // The jobs this window started, as novad last reported them, oldest
    // first.
    property var jobs: []
    readonly property var asking: Transfers.asking(jobs)

    // Copies or moves (a Protocol.TRANSFER_* mode) `sources` (absolute
    // paths) into the folder `folder`.
    function start(mode: string, sources: var, folder: string) {
        const job = FilesService.transfer(mode, sources, folder, error => {
            jobs = Transfers.apply(jobs, {
                job: job,
                mode: mode,
                state: Protocol.TRANSFER_FAILED,
                items: sources.length,
                name: Paths.folderName(sources[0]),
                done_bytes: 0,
                total_bytes: 0,
                error: error
            });
        });
        mine[job] = true;
    }

    // The numbers of the jobs this window started; others are other
    // windows'.
    property var mine: ({})

    Connections {
        target: FilesService
        function onTransferChanged(transfer: var) {
            if (!transfers.mine[transfer.job])
                return;
            transfers.jobs = Transfers.apply(transfers.jobs, transfer);
            if (Transfers.ended(transfer.state) && transfer.state !== Protocol.TRANSFER_FAILED)
                delete transfers.mine[transfer.job];
        }
    }

    // Holds the window's clicks while a clash waits for an answer.
    MouseArea {
        anchors.fill: parent
        visible: transfers.asking !== null
        acceptedButtons: Qt.AllButtons
        onWheel: wheel => wheel.accepted = true
    }

    Column {
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: Tokens.filesOverlayInset
        spacing: Tokens.gap

        Repeater {
            model: transfers.jobs

            TransferCard {
                required property var modelData

                transfer: modelData
                onCancelled: FilesService.cancelTransfer(modelData.job)
                onDismissed: {
                    delete transfers.mine[modelData.job];
                    transfers.jobs = Transfers.without(transfers.jobs, modelData.job);
                }
            }
        }
    }

    // The clash the dialog shows, kept while it fades out after an answer.
    property var shownClash: null
    onAskingChanged: {
        if (asking) {
            shownClash = asking.clash;
            dialog.forceActiveFocus();
        }
    }

    ClashDialog {
        id: dialog
        anchors.centerIn: parent
        enabled: transfers.asking !== null
        visible: opacity > 0 && transfers.shownClash !== null
        clash: transfers.shownClash ?? Transfers.NO_CLASH
        opacity: transfers.asking ? 1 : 0
        scale: transfers.asking ? 1 : Tokens.filesClashEnterScale
        onAnswered: (answer, all) => FilesService.answerClash(transfers.asking.job, transfers.asking.clash.id, answer, all)
        onCancelled: FilesService.cancelTransfer(transfers.asking.job)

        Behavior on opacity {
            NumberAnimation {
                duration: Tokens.durationPanel
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }
        Behavior on scale {
            NumberAnimation {
                duration: Tokens.durationPanel
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }
    }
}
