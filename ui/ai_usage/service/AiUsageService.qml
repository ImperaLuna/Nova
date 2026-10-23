pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// AI usage limits from novad, and the commands that change them. The pill
// and its panel (../AiUsage.qml) read and act through here, never through
// Daemon directly.
Singleton {
    id: root

    // The latest ai_usage event's data ({shown, providers}), or null.
    readonly property var usage: Daemon.aiUsage
    // The feature is on and novad has reported: the pill can show.
    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_AI_USAGE) && usage !== null

    // Shows `provider` ("claude", "codex") in the bar.
    function show(provider: string) {
        Daemon.call(Protocol.CMD_SHOW_AI_USAGE, {
            provider: provider
        });
    }

    // Asks novad for fresh numbers; it decides whether to fetch.
    function refresh() {
        Daemon.call(Protocol.CMD_REFRESH_AI_USAGE);
    }
}
