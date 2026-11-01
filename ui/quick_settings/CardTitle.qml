import QtQuick
import "../theme"

// A card's small uppercase heading, like "Resources".
Text {
    font.family: Tokens.fontSans
    font.pixelSize: Tokens.fontSizeSmall
    font.capitalization: Font.AllUppercase
    font.letterSpacing: Tokens.trackingLabel * Tokens.fontSizeSmall
    color: Tokens.fgSoft
}
