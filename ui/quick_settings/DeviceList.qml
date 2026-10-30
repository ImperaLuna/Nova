import QtQuick
import "devices.js" as Devices

// The Devices card's rows as a ListModel kept in step with novad's list, so
// a device that comes or goes fades on its own and a battery that changes
// eases to its new level, instead of every row being rebuilt. Roles are
// Devices.row's.
ListModel {
    id: list

    // Brings the model in line with `devices` (a peripherals event's),
    // keeping the rows whose device stays.
    function sync(devices: var) {
        for (let index = count - 1; index >= 0; index--) {
            const id = get(index).deviceId;
            if (!devices.some(device => device.id === id))
                remove(index);
        }
        devices.forEach((device, index) => {
            const entry = Devices.row(device);
            let at = -1;
            for (let other = index; other < count; other++) {
                if (get(other).deviceId === device.id)
                    at = other;
            }
            if (at < 0) {
                insert(index, entry);
            } else {
                if (at !== index)
                    move(at, index, 1);
                set(index, entry);
            }
        });
    }
}
