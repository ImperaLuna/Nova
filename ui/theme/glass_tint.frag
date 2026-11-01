#version 440
// Smoked glass's tint, as thick as each spot needs (GlassTint.qml): every
// pixel looks up how bright the screen is behind it (`screenCopy`, a small copy
// of the screen) and takes the least tint between minAlpha and maxAlpha that
// keeps ink of luminance `inkLum` at `contrast` : 1 over it. Until there is a
// copy (`ready` 0) it takes `fallback`. Nothing above `floorY` is looked at:
// the bar's row is Nova's own glass, so glass there looks just below it.
// `shape` 0 is a rounded rectangle (`radius`); 1 is a square with a circle
// cut out (`cut`: its centre and radius, in px), an edge tab's fillet.
layout(location = 0) in vec2 qt_TexCoord0;
layout(location = 0) out vec4 fragColor;

layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    vec4 tint;      // the glass color (rgb)
    vec4 region;    // this item on the screen copy: x, y, w, h, 0..1
    vec4 cut;       // shape 1: the cutout's centre (xy) and radius (z), px
    vec2 size;      // the item in px
    float radius;
    float shape;
    float floorY;
    float inkLum;
    float contrast;
    float minAlpha;
    float maxAlpha;
    float fallback;
    float ready;
};
layout(binding = 1) uniform sampler2D screenCopy;

float lin(float c) { return c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4); }
float lum(vec3 c) { return dot(vec3(lin(c.r), lin(c.g), lin(c.b)), vec3(0.2126, 0.7152, 0.0722)); }

void main() {
    float a = fallback;
    if (ready > 0.5) {
        vec2 at = region.xy + qt_TexCoord0 * region.zw;
        at.y = max(at.y, floorY);
        vec3 behind = texture(screenCopy, at).rgb;
        float darkest = (inkLum + 0.05) / contrast - 0.05;
        float lo = minAlpha;
        float hi = maxAlpha;
        if (lum(mix(behind, tint.rgb, lo)) <= darkest) {
            a = lo;
        } else {
            for (int i = 0; i < 7; i++) {
                float mid = 0.5 * (lo + hi);
                if (lum(mix(behind, tint.rgb, mid)) <= darkest) hi = mid; else lo = mid;
            }
            a = hi;
        }
    }
    vec2 p = qt_TexCoord0 * size;
    float cover;
    if (shape < 0.5) {
        // Rounded-rectangle coverage, antialiased over a pixel.
        vec2 q = abs(p - 0.5 * size) - (0.5 * size - vec2(radius));
        float dist = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
        cover = clamp(0.5 - dist, 0.0, 1.0);
    } else {
        cover = clamp(length(p - cut.xy) - cut.z + 0.5, 0.0, 1.0);
    }
    fragColor = vec4(tint.rgb, 1.0) * a * cover * qt_Opacity;
}
