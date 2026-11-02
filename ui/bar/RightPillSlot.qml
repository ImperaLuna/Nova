pragma ComponentBehavior: Bound

import QtQuick
import "../daemon"
import "../theme"
import "../tray"
import "../tray/service"
import "../slots/slots.js" as Slots

// The right pill in the bar.right row (view `right-pill`): the tray, and
// the right-pill.buttons host. There while it has a button or a tray app.
// It grows to the left as the tray opens and pushes the pills beside it
// along. A tray app with only a menu shows that menu (TrayMenu) below the
// pill.
BarSlot {
    id: slot

    pill: rightPill
    present: rightPill.hasButtons || rightPill.hasTray
    panels: [rightPill, trayMenu]

    // Opens the menu of the tray app `entry`, whose icon's middle is `at`
    // in the pill.
    function showTrayMenu(entry: var, at: point) {
        trayMenu.entry = entry;
        trayMenu.anchorX = rightPill.x + at.x;
        trayMenu.open = true;
    }

    RightPill {
        id: rightPill
        visible: slot.present
        x: slot.slotX
        y: Tokens.barGap
        width: implicitWidth
        height: implicitHeight
        buttons: PillButtons {
            stage: slot.stage
            slots: Slots.forHost(Daemon.features, Slots.HOST_RIGHT_PILL_BUTTONS)
        }
        trayApps: TrayService.shown
        trayAttention: TrayService.attention
        iconSource: entry => TrayService.iconSource(entry)
        // A click acts and closes the tray; an app that only has a menu
        // shows it instead.
        onTrayActivated: (entry, at) => {
            if (TrayService.opensMenu(entry)) {
                slot.showTrayMenu(entry, at);
            } else {
                TrayService.activate(entry);
                rightPill.open = false;
            }
        }
        onTrayMenuRequested: (entry, at) => slot.showTrayMenu(entry, at)
        onTraySecondaryActivated: entry => entry.item.secondaryActivate()
        onOpenChanged: {
            if (!open)
                trayMenu.open = false;
        }
    }

    TrayMenu {
        id: trayMenu
        screenWidth: slot.bar.width
        screenHeight: slot.bar.screenHeight
        iconSource: entry ? TrayService.iconSource(entry) : ""
        runEntry: run => TrayService.menuPicked(trayMenu.entry, run)
        onDone: rightPill.open = false
        onHideRequested: (key, hidden) => TrayService.hide(key, hidden)
    }
}
