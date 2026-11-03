.pragma library

// Who a thumbnail answer from novad goes to (FilesService). Each waiting
// ask is {modified, done, again}: the version of the file (its listing's
// time, ms) it wants. novad's answer ({path, modified_ms, thumbnail}) says
// which version it's for. Those who wanted another one ask once more, since
// the file changed between their ask and the answer; a file that's gone
// (no modified_ms) answers everyone, and so does a second answer, so a
// time that never matches can't ask forever.
function split(waiting, answer) {
    const known = answer.modified_ms !== undefined;
    const again = waiting.filter(wanted => known && wanted.modified !== answer.modified_ms && !wanted.again);
    return {
        answered: waiting.filter(wanted => !again.includes(wanted)),
        again: again
    };
}
