import QtQuick

// The central pill's workspaces as a ListModel kept in step with novad's
// list, so a Repeater over it adds, moves and drops one number at a time
// instead of rebuilding them all on every compositor event (which would
// restart their animations). Roles: workspaceId, name, active, urgent,
// special, elsewhere.
ListModel {
    id: list

    // The index of the workspace shown on the output, or -1.
    property int activeIndex: -1
    // How many special workspaces lead the list.
    property int specialCount: 0

    // Brings the model in line with `workspaces` (Protocol.barWorkspaces),
    // keeping the entries whose workspace stays.
    function sync(workspaces: var) {
        for (let index = count - 1; index >= 0; index--) {
            const id = get(index).workspaceId;
            if (!workspaces.some(workspace => workspace.id === id))
                remove(index);
        }
        let active = -1;
        workspaces.forEach((workspace, index) => {
            const entry = {
                workspaceId: workspace.id,
                name: workspace.name,
                active: workspace.active,
                urgent: workspace.urgent,
                special: workspace.special === true,
                elsewhere: workspace.shown_elsewhere === true
            };
            let at = -1;
            for (let other = index; other < count; other++) {
                if (get(other).workspaceId === workspace.id)
                    at = other;
            }
            if (at < 0) {
                insert(index, entry);
            } else {
                if (at !== index)
                    move(at, index, 1);
                set(index, entry);
            }
            if (workspace.active)
                active = index;
        });
        activeIndex = active;
        specialCount = workspaces.filter(workspace => workspace.special).length;
    }
}
