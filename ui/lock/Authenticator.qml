import QtQuick
import Quickshell
import Quickshell.Services.Pam
import "../theme"
import "service"

// The lock screen's password check: the keys typed on any screen's surface,
// PAM, and unlocking (LockService) once the password checks out. The
// password typed so far lives here, shared by every screen, so whichever
// surface the compositor sends the keys to types into it.
Scope {
    id: root

    readonly property string userName: Quickshell.env("USER") ?? Quickshell.env("LOGNAME") ?? ""
    // Characters typed so far. Cleared once handed to PAM.
    property string password: ""
    property bool failed: false
    property string message: ""
    // The password checked out: the prompt fades, then the lock lets go.
    property bool unlocking: false
    readonly property bool checking: pam.active

    // A wrong password: every screen's prompt shakes.
    signal rejected

    // A key pressed on any screen's surface.
    function key(event: KeyEvent) {
        if (checking || unlocking)
            return;
        if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
            submit();
        } else if (event.key === Qt.Key_Escape) {
            password = "";
        } else if (event.key === Qt.Key_Backspace) {
            password = password.slice(0, -1);
        } else if (isPrintable(event.text)) {
            password += event.text;
            failed = false;
            message = "";
        }
    }

    function isPrintable(text: string): bool {
        const control = 0x20, del = 0x7f;
        return text.length > 0 && Array.from(text).every(char => char.charCodeAt(0) >= control && char.charCodeAt(0) !== del);
    }

    function submit() {
        if (password === "")
            return;
        failed = false;
        message = "";
        if (!pam.start())
            fail(qsTr("Couldn't check the password"), "PAM didn't start");
    }

    function fail(shown: string, logged: string) {
        password = "";
        failed = true;
        message = shown;
        console.warn(`nova: lock screen: ${logged}`);
        rejected();
    }

    PamContext {
        id: pam

        // ./pam/nova-lock: the login password, wherever the distro keeps it.
        configDirectory: "pam"
        config: "nova-lock"

        onResponseRequiredChanged: {
            if (!responseRequired)
                return;
            respond(root.password);
            root.password = "";
        }
        onCompleted: result => {
            if (result === PamResult.Success) {
                root.unlocking = true;
                unlockAfterFade.start();
            } else if (result === PamResult.Failed) {
                root.fail(qsTr("Wrong password"), "wrong password");
            } else if (result === PamResult.MaxTries) {
                root.fail(qsTr("Too many tries. Wait a moment, then try again"), "PAM's tries ran out");
            } else {
                root.fail(qsTr("Couldn't check the password"), `PAM: ${PamResult.toString(result)} ${message}`);
            }
        }
    }

    Timer {
        id: unlockAfterFade
        interval: Tokens.durationUnlock
        onTriggered: {
            LockService.unlocked();
            root.unlocking = false;
        }
    }
}
