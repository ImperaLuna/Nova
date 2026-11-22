// The soft shadow under a piece of glass, cut out where the glass is: the
// glass is see-through, and a shadow under it would only darken it.
// `glass.rs` paints it over the glass's bounds grown by the shadow's room
// on every side, so the glass sits in the middle of this rectangle.
//
// quad.params, all in device pixels:
//   [0] the glass: half its width, half its height, its corner radius
//   [1] the shadow: how far below the glass it falls, its blur's sigma
//   [2] its color (straight alpha)

const PI: f32 = 3.14159265;
// How many strips the blur is summed over, top to bottom. GPUI's own shadow
// uses as few (gpui_wgpu's fs_shadow, which this follows).
const STRIPS: i32 = 4;
// The blur is summed out to this many sigmas.
const REACH: f32 = 3.0;

fn gaussian(x: f32, sigma: f32) -> f32 {
    return exp(-(x * x) / (2.0 * sigma * sigma)) / (sqrt(2.0 * PI) * sigma);
}

// An approximation of the error function, to a vec2 at once.
fn erf(v: vec2<f32>) -> vec2<f32> {
    let s = sign(v);
    let a = abs(v);
    let r1 = 1.0 + (0.278393 + (0.230389 + (0.000972 + 0.078108 * a) * a) * a) * a;
    let r2 = r1 * r1;
    return s - s / (r2 * r2);
}

// How much of the rounded rectangle's row at height `y` a blur along x
// covers at `x`.
fn blur_along_x(x: f32, y: f32, sigma: f32, corner: f32, half: vec2<f32>) -> f32 {
    let delta = min(half.y - corner - abs(y), 0.0);
    let curved = half.x - corner + sqrt(max(0.0, corner * corner - delta * delta));
    let integral = 0.5 + 0.5 * erf((x + vec2<f32>(-curved, curved)) * (sqrt(0.5) / sigma));
    return integral.y - integral.x;
}

// Distance from `p` to the edge of a rounded rectangle about the origin:
// below zero inside it.
fn rounded_distance(p: vec2<f32>, half: vec2<f32>, corner: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(corner);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - corner;
}

fn shade(p: vec2<f32>, size: vec2<f32>) -> vec4<f32> {
    let half = quad.params[0].xy;
    let corner = quad.params[0].z;
    let fall = quad.params[1].x;
    let sigma = quad.params[1].y;
    let color = quad.params[2];

    let from_glass = p - size * 0.5;
    // 0 on the glass, 1 off it, over one pixel at its edge.
    let off_glass = saturate(rounded_distance(from_glass, half, corner) + 0.5);

    // The shadow is the glass's shape, moved down and blurred.
    let at = from_glass - vec2<f32>(0.0, fall);
    let low = at.y - half.y;
    let high = at.y + half.y;
    let start = clamp(-REACH * sigma, low, high);
    let end = clamp(REACH * sigma, low, high);
    let step = (end - start) / f32(STRIPS);
    var y = start + step * 0.5;
    var cover = 0.0;
    for (var strip = 0; strip < STRIPS; strip += 1) {
        cover += blur_along_x(at.x, at.y - y, sigma, corner, half) * gaussian(y, sigma) * step;
        y += step;
    }

    let alpha = color.a * cover * off_glass;
    return vec4<f32>(color.rgb * alpha, alpha);
}
