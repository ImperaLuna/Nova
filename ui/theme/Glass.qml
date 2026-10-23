import QtQuick
import QtQuick.Shapes

// Nova's liquid glass. The compositor blurs whatever is behind it (the
// window's BackgroundEffect region); this paints on top of that blur: a light
// tint, a sheen that brightens the top and a shade along the bottom, and a
// rim that catches the light at the top-left and bottom-right corners.
// Children go on top.
Rectangle {
    id: glass

    color: Tokens.glass

    // Sheen at the top, fading out, then a shade along the bottom edge.
    Rectangle {
        anchors.fill: parent
        radius: glass.radius
        gradient: Gradient {
            GradientStop {
                position: 0
                color: Tokens.glassSheen
            }
            GradientStop {
                position: 0.45
                color: Tokens.glassVeil
            }
            GradientStop {
                position: 0.85
                color: "transparent"
            }
            GradientStop {
                position: 1
                color: Tokens.glassShade
            }
        }
    }

    // The rim: a ring between the outer edge and one inset by its width,
    // lit diagonally.
    Shape {
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            fillRule: ShapePath.OddEvenFill
            strokeColor: "transparent"
            fillGradient: LinearGradient {
                x1: 0
                y1: 0
                x2: glass.width
                y2: glass.height
                GradientStop {
                    position: 0
                    color: Tokens.glassRimLit
                }
                GradientStop {
                    position: 0.28
                    color: Tokens.glassRim
                }
                GradientStop {
                    position: 0.5
                    color: Tokens.glassRimDim
                }
                GradientStop {
                    position: 0.72
                    color: Tokens.glassRim
                }
                GradientStop {
                    position: 1
                    color: Tokens.glassRimGlow
                }
            }

            PathRectangle {
                width: glass.width
                height: glass.height
                radius: glass.radius
            }
            PathRectangle {
                x: Tokens.glassRimWidth
                y: Tokens.glassRimWidth
                width: glass.width - 2 * Tokens.glassRimWidth
                height: glass.height - 2 * Tokens.glassRimWidth
                radius: Math.max(0, glass.radius - Tokens.glassRimWidth)
            }
        }
    }
}
