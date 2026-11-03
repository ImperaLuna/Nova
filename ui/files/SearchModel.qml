import QtQml
import "../theme"
import "search.js" as Search

// A Nova Files window's search: what's typed (`query`) asked of novad's
// name index as it changes, below the folder `within` or everywhere when
// that's "". Typing settles for Tokens.filesSearchSettle before novad is
// asked, and an answer to anything but the latest ask is dropped, so is
// one that comes while new typing settles. A search asked while the index
// was still being built is asked again when novad says it's built
// (`searchReady`), and one novad couldn't answer when it's connected
// again.
QtObject {
    id: search

    property string query: ""
    property string within: ""
    // What asks novad: FilesService, or a stand-in in tests (`search()`,
    // `connected`, `searchReady`).
    property var service: null
    // How long typing settles first; a property so tests can shorten it.
    property int settle: Tokens.filesSearchSettle

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
            rows = [];
            total = 0;
            return;
        }
        const ask = ++_asks;
        service.search(query.trim(), within, found => {
            if (ask !== _asks)
                return;
            // novad couldn't search (it's gone, say): no count.
            answered = found !== null;
            ready = found?.ready ?? ready;
            rows = (found?.results ?? []).map(Search.row);
            total = found?.total ?? 0;
        });
    }

    // What was asked changed: answers on their way are for something else.
    function _changed() {
        _asks++;
        answered = false;
    }

    onQueryChanged: {
        _changed();
        if (query.trim() !== "")
            _settling.restart();
        else
            _ask();
    }
    onWithinChanged: {
        _changed();
        _ask();
    }

    readonly property Timer _settling: Timer {
        interval: search.settle
        onTriggered: search._ask()
    }
    readonly property Connections _service: Connections {
        target: search.service
        function onSearchReady() {
            if (search.active && !search.ready)
                search._ask();
        }
        function onConnectedChanged() {
            if (search.service.connected && search.active && !search.answered)
                search._ask();
        }
    }
}
