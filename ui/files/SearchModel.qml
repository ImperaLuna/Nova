import QtQml
import "../theme"
import "search.js" as Search

// A Nova Files window's search: what's typed (`query`) asked of novad's
// name index as it changes, below the folder `within` or everywhere when
// that's "". Typing settles for Tokens.filesSearchSettle before novad is
// asked, and an answer to anything but the latest ask is dropped. While
// the index is still being built it asks again every
// Tokens.filesSearchRetry, so results come in once it is.
QtObject {
    id: search

    property string query: ""
    property string within: ""
    // What asks novad: FilesService, or a stand-in in tests.
    property var service: null
    // How long typing settles first, and how often to ask again while the
    // index is built; properties so tests can shorten them.
    property int settle: Tokens.filesSearchSettle
    property int retry: Tokens.filesSearchRetry

    // Search.row of each result, best first.
    property var rows: []
    property int total: 0
    // novad's index is built (also until a first answer says otherwise).
    property bool ready: true
    // An answer came for what's asked now.
    property bool answered: false
    readonly property bool active: query.trim() !== ""

    property int _asks: 0

    function _ask() {
        // Not `active`: in a change handler, it may not have caught up yet.
        if (query.trim() === "") {
            _asks++;
            rows = [];
            total = 0;
            answered = false;
            _waiting.running = false;
            return;
        }
        const ask = ++_asks;
        service.search(query.trim(), within, found => {
            if (ask !== _asks)
                return;
            answered = true;
            ready = found?.ready ?? true;
            rows = (found?.results ?? []).map(Search.row);
            total = found?.total ?? 0;
            _waiting.running = !ready;
        });
    }

    onQueryChanged: {
        answered = false;
        if (query.trim() !== "")
            _settling.restart();
        else
            _ask();
    }
    onWithinChanged: {
        answered = false;
        _ask();
    }

    readonly property Timer _settling: Timer {
        interval: search.settle
        onTriggered: search._ask()
    }
    readonly property Timer _waiting: Timer {
        interval: search.retry
        onTriggered: search._ask()
    }
}
