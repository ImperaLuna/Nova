#version 440
// Fades the item it draws (as its layer.effect) out towards its
// left and right edges: over `fadeStart` px from the left and `fadeEnd` px
// from the right, none where either is 0. For a long title, cut at the
// pill's edge (MarqueeTitle.qml).
layout(location = 0) in vec2 qt_TexCoord0;
layout(location = 0) out vec4 fragColor;

layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    float width;     // the item's width in px
    float fadeStart; // px faded at the left edge
    float fadeEnd;   // px faded at the right edge
};
layout(binding = 1) uniform sampler2D source;

void main() {
    float x = qt_TexCoord0.x * width;
    float fromStart = fadeStart > 0.0 ? clamp(x / fadeStart, 0.0, 1.0) : 1.0;
    float fromEnd = fadeEnd > 0.0 ? clamp((width - x) / fadeEnd, 0.0, 1.0) : 1.0;
    fragColor = texture(source, qt_TexCoord0) * fromStart * fromEnd * qt_Opacity;
}
