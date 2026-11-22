// The volume capsule (ui/volume/liquid.frag) ported to WGSL for the fork's
// custom shader patch. Same math; derivatives are taken up front because
// WGSL only allows them in uniform control flow, so there are no early
// returns. All sizes are device pixels.
//
// quad.params layout (vec4s):
//  0 box x, y, w, h        1 radius, wall, level, time
//  2 rise, swell, amp, tilt 3 meniscus, meniscusReach, fizz, specks
//  4 speckRadius, speckRadiusRange, speckGlow, speckGlowMax
//  5 glow, glowDepth, streak, streakFade
//  6 streakAt, streakWidth, rimOn, rimWidth
//  7 rimShade, sheenFade, shadowBlur, shadowOffset
//  8 tint  9 sheen  10 rimLit  11 liquidColor  12 shadow  13 light

fn sd_box(p: vec2<f32>, c: vec2<f32>, he: vec2<f32>, r: f32) -> f32 {
    let q = abs(p - c) - he + r;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn hash(n: f32) -> f32 {
    return fract(sin(n * 127.1) * 43758.5453);
}

fn over(top: vec4<f32>, under: vec4<f32>) -> vec4<f32> {
    return top + under * (1.0 - top.a);
}

fn pm(c: vec4<f32>, a: f32) -> vec4<f32> {
    return vec4<f32>(c.rgb * c.a * a, c.a * a);
}

fn shade(p: vec2<f32>, size: vec2<f32>) -> vec4<f32> {
    let box = quad.params[0];
    let radius = quad.params[1].x;
    let wall = quad.params[1].y;
    let level = quad.params[1].z;
    let time = quad.params[1].w;
    let rise = quad.params[2].x;
    let swell = quad.params[2].y;
    let amp = quad.params[2].z;
    let tilt = quad.params[2].w;
    let meniscus = quad.params[3].x;
    let meniscus_reach = quad.params[3].y;
    let fizz = quad.params[3].z;
    let specks = quad.params[3].w;
    let speck_radius = quad.params[4].x;
    let speck_radius_range = quad.params[4].y;
    let speck_glow = quad.params[4].z;
    let speck_glow_max = quad.params[4].w;
    let glow = quad.params[5].x;
    let glow_depth = quad.params[5].y;
    let streak = quad.params[5].z;
    let streak_fade = quad.params[5].w;
    let streak_at = quad.params[6].x;
    let streak_width = quad.params[6].y;
    let rim_on = quad.params[6].z;
    let rim_width = quad.params[6].w;
    let rim_shade = quad.params[7].x;
    let sheen_fade = quad.params[7].y;
    let shadow_blur = quad.params[7].z;
    let shadow_offset = quad.params[7].w;
    let tint = quad.params[8];
    let sheen = quad.params[9];
    let rim_lit = quad.params[10];
    let liquid_color = quad.params[11];
    let shadow = quad.params[12];
    let light = quad.params[13];

    let he = box.zw * 0.5;
    let c = box.xy + he;
    let d = sd_box(p, c, he, radius);
    let aa = max(fwidth(d), 1e-3);
    let g = normalize(vec2<f32>(dpdx(d), dpdy(d)) + vec2<f32>(1e-5));
    let inside = clamp(0.5 - d / aa, 0.0, 1.0);

    // The liquid's surface, needed for its derivative before any branch.
    let d_in = d + wall;
    let top = box.y + wall;
    let span = box.w - 2.0 * wall;
    let inner = max(box.z - 2.0 * wall, 1.0);
    let u = clamp((p.x - box.x - wall) / inner, 0.0, 1.0);
    var surf = top + (1.0 - level) * span
        + swell * (sin(6.2832 * 0.55 * u + time * 1.3) + 0.45 * sin(6.2832 * 0.3 * u - time * 0.9 + 1.7))
        + amp * cos(3.1416 * u) * sin(time * 6.5)
        + tilt * (p.x - c.x);
    surf = surf - meniscus * exp(-max(-d_in, 0.0) / meniscus_reach);
    let aas = max(fwidth(p.y - surf), 1e-3) * 1.5;

    // Shadow, outside the glass only.
    let ds = max(sd_box(p, c + vec2<f32>(0.0, shadow_offset), he, radius), 0.0);
    let fall = ds / max(shadow_blur * 0.5, 1.0);
    let under = pm(shadow, exp(-fall * fall) * (1.0 - inside));

    // Glass: tint, sheen from the top, a rim lit from the top left.
    let v = clamp((p.y - box.y) / max(box.w, 1.0), 0.0, 1.0);
    var col = pm(tint, 1.0);
    col = over(pm(sheen, 1.0 - smoothstep(0.0, sheen_fade, v)), col);
    let rim = 1.0 - smoothstep(0.0, rim_width, abs(d + rim_width * 0.5));
    let lit = mix(rim_shade, 1.0, clamp(dot(-g, normalize(vec2<f32>(-1.0, -1.0))) * 0.5 + 0.5, 0.0, 1.0));
    col = over(pm(rim_lit, rim * lit * rim_on), col);

    let in_capsule = clamp(0.5 - d_in / aa, 0.0, 1.0);
    let wet = clamp(0.5 - (surf - p.y) / aas, 0.0, 1.0) * in_capsule * smoothstep(0.0, 0.03, level);
    let below = max(p.y - surf, 0.0);

    let depth = clamp(below / max(span, 1.0), 0.0, 1.0);
    var lq = liquid_color.rgb * mix(1.08, 0.78, smoothstep(0.0, 1.0, depth));
    lq = lq * mix(1.0, 0.86, smoothstep(0.0, 0.5, abs(u - 0.5)));
    lq = mix(lq, light.rgb, glow * exp(-below / glow_depth));
    let side = (u - streak_at) / streak_width;
    lq = mix(lq, light.rgb, streak * exp(-side * side) * smoothstep(0.0, streak_fade, below));
    col = over(pm(vec4<f32>(min(lq, vec3<f32>(1.0)), liquid_color.a), wet), col);

    let bottom = box.y + box.w - wall;
    let left = box.x + wall;
    var motes = 0.0;
    for (var i = 0; i < 32; i = i + 1) {
        let fi = f32(i);
        let on = clamp(fizz * specks - fi, 0.0, 1.0);
        if (on <= 0.0) {
            break;
        }
        let pace = 0.05 + 0.07 * hash(fi + 7.3);
        let t = rise * pace + hash(fi + 1.0);
        let ph = fract(t);
        let n = fi * 31.7 + floor(t) * 17.3;
        let lane = hash(n + 1.0);
        let drift = hash(n + 2.0) - 0.5;
        let wobble = hash(n + 3.0);
        let size_n = hash(n + 4.0);
        let y = bottom - ph * level * span;
        let x = left + inner * (0.12 + 0.76 * lane + 0.35 * drift * ph)
            + (1.0 + 2.5 * wobble) * sin(ph * (6.0 + 10.0 * wobble) + 6.2832 * size_n)
            + tilt * (bottom - y) * 0.5;
        let at = vec2<f32>(clamp(x, left + 2.0, left + inner - 2.0), y);
        let r = speck_radius + speck_radius_range * size_n;
        let dm = length(p - at);
        let twinkle = 0.55 + 0.45 * sin(time * (1.5 + 2.0 * lane) + fi * 2.3);
        motes = motes + exp(-dm * dm / (r * r)) * twinkle * smoothstep(0.0, 0.08, ph) * on;
    }
    motes = motes * clamp(below / 4.0, 0.0, 1.0);
    col = over(pm(light, clamp(motes * speck_glow, 0.0, speck_glow_max) * wet), col);

    return select(over(col * inside, under), under, inside <= 0.0);
}
