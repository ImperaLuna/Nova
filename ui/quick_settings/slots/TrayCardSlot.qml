import QtQuick
import ".."
import "../../tray/service"

// tray.card: every tray app. A click on one with a menu shows it over the
// sidebar; any other runs the app and closes quick settings.
QsSlot {
    id: slot

    present: TrayService.all.length > 0
    implicitHeight: card.implicitHeight

    TrayCard {
        id: card
        width: slot.width
        apps: TrayService.all
        iconSource: entry => TrayService.iconSource(entry)
        onActivated: entry => {
            if (TrayService.opensMenu(entry)) {
                slot.qs.trayMenuRequested(entry, null);
            } else {
                TrayService.activate(entry);
                slot.qs.closeRequested();
            }
        }
        onMenuRequested: (entry, at) => slot.qs.trayMenuRequested(entry, card.mapToItem(slot.qs.menuArea, at.x, at.y))
        onSecondaryActivated: entry => entry.item.secondaryActivate()
    }
}
