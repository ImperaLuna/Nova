pragma ComponentBehavior: Bound

import QtQuick
import "../bar"
import "../theme"
import "service"

// The AI usage pill in the bar.left row (view `ai-usage.pill`), once its
// service has numbers. Open, it grows over the pills beside it.
BarSlot {
    id: slot

    readonly property AiUsage aiUsage: loader.item as AiUsage

    pill: aiUsage
    closedWidth: aiUsage?.pillWidth ?? 0
    panels: aiUsage ? [aiUsage] : []

    Loader {
        id: loader
        y: Tokens.barGap
        active: AiUsageService.ready
        onLoaded: console.info("nova: ai usage pill shown")
        sourceComponent: AiUsage {
            // Placed itself, not by the Loader: its blur and clicks only
            // follow its own moves.
            x: slot.slotX
            usage: AiUsageService.usage
            onShowRequested: provider => AiUsageService.show(provider)
            onRefreshRequested: AiUsageService.refresh()
        }
    }
}
