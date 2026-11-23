// One card of the wallpaper carousel, turned about its upright middle line
// and seen in perspective: its picture cut to the card's rounding, dimmed,
// and ringed while it is the picked one. `tilt.rs` holds the same sums and
// what they follow; `draw.rs` paints this over the box the turned card
// fills and appends `picture`, which reads the image or the quiet color of
// a card still loading.
//
// quad.params, lengths in device pixels:
//   [0] the card's middle in the box (x, y), half its width, half its height
//   [1] its corner radius, its ring's width (0 for none), how much narrower
//       the turn makes it, how fast it moves away from the eye per pixel
//       from the middle line
//   [2] its opacity, how much of the dim color lies over the picture
//   [3] the dim color, [4] the ring's color, [5] the color of a card still
//       loading (all straight alpha)

// The picture is read this many times across each pixel: turned away, a
// pixel covers up to three of the picture's.
const TAPS: i32 = 4;

// The card's point under a point of the screen (both from the card's
// middle); far outside the card past where its plane can be seen.
fn onto_card(at: vec2<f32>) -> vec2<f32> {
    let narrow = quad.params[1].z;
    let lean = quad.params[1].w;
    let reach = narrow - lean * at.x;
    if (reach <= 1e-4) {
        return vec2<f32>(1e6);
    }
    let x = at.x / reach;
    return vec2<f32>(x, at.y * (1.0 + lean * x));
}

// Distance from `p` to the edge of a rounded rectangle about the origin:
// below zero inside.
fn rounded(p: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

// `top` (straight alpha) at `opacity` over `under` (premultiplied).
fn over(top: vec4<f32>, opacity: f32, under: vec4<f32>) -> vec4<f32> {
    let alpha = top.a * opacity;
    return vec4<f32>(top.rgb * alpha, alpha) + under * (1.0 - alpha);
}

fn shade(p: vec2<f32>, size: vec2<f32>) -> vec4<f32> {
    let half = quad.params[0].zw;
    let radius = quad.params[1].x;
    let ring = quad.params[1].y;
    let at = p - quad.params[0].xy;
    let here = onto_card(at);

    // How much of this pixel the card covers, and its ring just outside.
    let distance = rounded(here, half, radius);
    let pixel = max(fwidth(distance), 1e-4);
    let card = saturate(0.5 - distance / pixel);
    let ringed = saturate(0.5 - (distance - ring) / pixel) - card;

    var seen = vec4<f32>(0.0);
    for (var tap = 0; tap < TAPS; tap++) {
        let across = (f32(tap) + 0.5) / f32(TAPS) - 0.5;
        let spot = onto_card(at + vec2<f32>(across, 0.0));
        seen += picture(spot / (2.0 * half) + vec2<f32>(0.5));
    }
    seen /= f32(TAPS);

    let lit = vec4<f32>(seen.rgb * seen.a, seen.a);
    let dimmed = over(quad.params[3], quad.params[2].y, lit);
    let rim = quad.params[4];
    let drawn = dimmed * card + vec4<f32>(rim.rgb * rim.a, rim.a) * ringed;
    return drawn * quad.params[2].x;
}
