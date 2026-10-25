import QtQuick
import "../theme"
import "volume.js" as Volume

// The motion behind the capsule: springs for how far it's out and for the
// liquid's level, and the liquid's reaction to both. Sliding sideways tilts
// the surface (it lags the glass); a level change makes it slosh and sends
// more specks up. Ticks only while the capsule is out or moving, so a
// hidden one costs nothing.
QtObject {
    id: motion

    // Targets: 1 out, 0 in; the fill, 0..1.
    property real openTarget: 0
    property real levelTarget: 0
    // A muted or missing output: the surface goes still, no specks.
    property bool still: false

    // Outputs, for the shader and the label.
    property real open: 0
    property real openSpeed: 0
    property real level: levelTarget
    property real time: 0
    // How far the specks have risen: time, run faster while it fizzes.
    property real rise: 0
    property real amp: 0
    property real tilt: 0
    property real fizz: 0

    property real levelSpeed: 0
    property real tiltSpeed: 0
    property real slosh: 0

    // Out, or on its way in or out.
    readonly property bool shown: open > 0.002 || openTarget > 0
    // While shown the surface keeps moving, so it ticks the whole time.
    readonly property bool ticking: shown

    readonly property FrameAnimation frames: FrameAnimation {
        running: motion.ticking
        onTriggered: motion.advance(Math.min(frameTime, 1 / 30))
    }

    // `dt` is capped by the caller, so a frame after a pause doesn't throw
    // the springs.
    function advance(dt: real) {
        time += dt;
        const out = Volume.spring(open, openSpeed, openTarget, Tokens.springCapsule, Tokens.springCapsuleDamping, dt);
        open = out.x;
        openSpeed = out.v;
        const fill = Volume.spring(level, levelSpeed, levelTarget, Tokens.springLevel, Tokens.springLevelDamping, dt);
        level = Volume.clamp(fill.x, 0, 1);
        levelSpeed = fill.v;
        // The glass's sideways acceleration tilts the surface, which swings
        // back like a pendulum.
        const swing = Volume.spring(tilt, tiltSpeed, 0, Tokens.springTilt, Tokens.springTiltDamping, dt);
        tiltSpeed = swing.v - out.a * Tokens.liquidTiltGain * dt;
        tilt = Volume.clamp(swing.x, -Tokens.liquidTiltMax, Tokens.liquidTiltMax);
        // Level changes stir up waves, which settle.
        slosh = Math.min(Tokens.liquidSloshMax, slosh * Math.exp(-dt / Tokens.liquidSloshDecay) + Math.abs(fill.v) * Tokens.liquidSloshGain * dt);
        amp = (still ? 0 : Tokens.liquidIdleWave) + slosh;
        fizz = still ? 0 : Math.min(1, Tokens.liquidFizz + slosh / Tokens.liquidSloshMax + Math.abs(tilt) / Tokens.liquidTiltMax);
        rise += dt * (1 + Tokens.liquidFizzPace * fizz);
    }
}
