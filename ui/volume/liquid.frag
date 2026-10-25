#version 440
// The volume capsule (Capsule.qml) in one pass: a glass capsule with a
// liquid fill and a soft shadow, from the IMP-56 spike. The surface is one
// slow swell longer than the capsule is wide, plus the capsule's first
// sloshing mode when stirred, so it stays a smooth curve and never ripples.
// Every color and size comes in as a uniform from Tokens.qml; the numbers
// below are only the shape of the waves and the light inside the liquid.
layout(location = 0) in vec2 qt_TexCoord0;
layout(location = 0) out vec4 fragColor;

layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    vec2 size;        // item size in px
    vec4 box;         // capsule: x, y, w, h in px
    float radius;     // capsule corner radius
    float wall;       // glass between the rim and the liquid, px
    float level;      // 0..1 fill
    float time;       // seconds
    float rise;       // how far the specks have risen: seconds, quicker while it fizzes
    float swell;      // the slow swell's height, px (0: a still surface)
    float amp;        // sloshing wave amplitude, px
    float tilt;       // surface tilt, px per px
    float meniscus;   // how far the surface creeps up the walls, px
    float fizz;       // 0..1: how many specks rise
    float specks;     // most specks at once
    float glow;       // light just under the surface, 0..1
    float streak;     // light down the left side, 0..1
    float rimOn;      // 1: lit rim, 0: none (the dashed outline replaces it)
    float rimWidth;   // px
    float rimShade;   // the rim's brightness on its unlit side, 0..1
    float sheenFade;  // where, top to bottom, the sheen has faded, 0..1
    float shadowBlur; // px
    float shadowOffset; // px, downwards
    vec4 tint;        // glass
    vec4 sheen;       // glass sheen at the top
    vec4 rimLit;
    vec4 liquidColor;
    vec4 shadow;
};

float sdBox(vec2 p, vec2 c, vec2 he, float r) {
    vec2 q = abs(p - c) - he + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

float hash(float n) { return fract(sin(n * 127.1) * 43758.5453); }

vec4 over(vec4 top, vec4 under) { // premultiplied
    return top + under * (1.0 - top.a);
}

vec4 pm(vec4 c, float a) { return vec4(c.rgb * c.a * a, c.a * a); }

void main() {
    vec2 p = qt_TexCoord0 * size;
    vec2 he = box.zw * 0.5;
    vec2 c = box.xy + he;
    float d = sdBox(p, c, he, radius);
    float aa = max(fwidth(d), 1e-3);
    float inside = clamp(0.5 - d / aa, 0.0, 1.0);

    // The shadow, outside the glass only.
    float ds = max(sdBox(p, c + vec2(0.0, shadowOffset), he, radius), 0.0);
    float fall = ds / max(shadowBlur * 0.5, 1.0);
    vec4 under = pm(shadow, exp(-fall * fall) * (1.0 - inside));
    if (inside <= 0.0) {
        fragColor = under * qt_Opacity;
        return;
    }

    // Glass: tint, a sheen from the top fading out, a rim lit from the top-left.
    float v = clamp((p.y - box.y) / max(box.w, 1.0), 0.0, 1.0);
    vec4 col = pm(tint, 1.0);
    col = over(pm(sheen, 1.0 - smoothstep(0.0, sheenFade, v)), col);
    vec2 g = normalize(vec2(dFdx(d), dFdy(d)) + 1e-5);
    float rim = 1.0 - smoothstep(0.0, rimWidth, abs(d + rimWidth * 0.5));
    float lit = mix(rimShade, 1.0, clamp(dot(-g, normalize(vec2(-1.0, -1.0))) * 0.5 + 0.5, 0.0, 1.0));
    col = over(pm(rimLit, rim * lit * rimOn), col);

    // Liquid: inside the capsule inset by the wall, below its surface.
    float dIn = d + wall;
    float top = box.y + wall;
    float span = box.w - 2.0 * wall;
    float inner = max(box.z - 2.0 * wall, 1.0);
    float u = clamp((p.x - box.x - wall) / inner, 0.0, 1.0);
    float surf = top + (1.0 - level) * span
        + swell * (sin(6.2832 * 0.55 * u + time * 1.3) + 0.45 * sin(6.2832 * 0.3 * u - time * 0.9 + 1.7))
        + amp * cos(3.1416 * u) * sin(time * 6.5)
        + tilt * (p.x - c.x);
    surf -= meniscus * exp(-max(-dIn, 0.0) / 5.0);
    float aas = max(fwidth(p.y - surf), 1e-3) * 1.5;
    float inCapsule = clamp(0.5 - dIn / aa, 0.0, 1.0);
    // An empty capsule shows no sliver of liquid at the bottom.
    float wet = clamp(0.5 - (surf - p.y) / aas, 0.0, 1.0) * inCapsule * smoothstep(0.0, 0.03, level);
    float below = max(p.y - surf, 0.0);

    // Body: light just under the surface, deeper and richer further down,
    // shaded towards the walls like a rounded volume, with a soft streak of
    // light down the left side.
    float depth = clamp(below / max(span, 1.0), 0.0, 1.0);
    vec3 lq = liquidColor.rgb * mix(1.08, 0.78, smoothstep(0.0, 1.0, depth));
    lq *= mix(1.0, 0.86, smoothstep(0.0, 0.5, abs(u - 0.5)));
    lq = mix(lq, vec3(1.0), glow * exp(-below / 6.0));
    float side = (u - 0.18) / 0.07;
    lq = mix(lq, vec3(1.0), streak * exp(-side * side) * smoothstep(0.0, 18.0, below));
    col = over(pm(vec4(min(lq, vec3(1.0)), liquidColor.a), wet), col);

    // Specks: soft glowing motes rising from the bottom to the surface,
    // where they fade. Each one rises at its own pace, and every trip up starts
    // from a new spot with a new size, drift and wobble, so they never
    // retrace a path. Their position follows `rise`, which only ever grows,
    // so a level change or more fizz speeds them up without making them jump.
    float bottom = box.y + box.w - wall;
    float left = box.x + wall;
    float motes = 0.0;
    for (int i = 0; i < 32; i++) {
        float fi = float(i);
        float on = clamp(fizz * specks - fi, 0.0, 1.0);
        if (on <= 0.0)
            break;
        float pace = 0.05 + 0.07 * hash(fi + 7.3);
        float t = rise * pace + hash(fi + 1.0);
        float ph = fract(t);
        float n = fi * 31.7 + floor(t) * 17.3;
        float lane = hash(n + 1.0), drift = hash(n + 2.0) - 0.5;
        float wobble = hash(n + 3.0), size = hash(n + 4.0);
        float y = bottom - ph * level * span;
        float x = left + inner * (0.12 + 0.76 * lane + 0.35 * drift * ph)
            + (1.0 + 2.5 * wobble) * sin(ph * (6.0 + 10.0 * wobble) + 6.2832 * size)
            + tilt * (bottom - y) * 0.5;
        vec2 at = vec2(clamp(x, left + 2.0, left + inner - 2.0), y);
        float r = 0.8 + 1.5 * size;
        float dm = length(p - at);
        float twinkle = 0.55 + 0.45 * sin(time * (1.5 + 2.0 * lane) + fi * 2.3);
        motes += exp(-dm * dm / (r * r)) * twinkle * smoothstep(0.0, 0.08, ph) * on;
    }
    motes *= clamp(below / 4.0, 0.0, 1.0);
    col = over(vec4(1.0) * clamp(motes * 0.7, 0.0, 0.85) * wet, col);

    fragColor = over(col * inside, under) * qt_Opacity;
}
