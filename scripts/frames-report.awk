# Missed vblanks per screen, from the UI's `nova-frame:` lines
# (ui/stage/FrameLog.qml, NOVA_FRAME_LOG=1) in any log on stdin or in the
# files named. `just frames` runs it on a live desktop; the VM tests print it.
#
# A frame that comes more than LATE vblanks after the one before missed a
# vblank: the screen showed the old frame again. A frame more than
# BURST_GAP_MS after the one before starts a new burst (an animation after
# idle), so its gap is idle time, not frame time. A screen with no frames
# but bursts' first ones (a lone redraw when another screen animates) has
# nothing to report and is left out.
#
# -v refresh="DP-1=144 DP-2=120": each screen's refresh rate in Hz; `*=60`
#    stands for every screen not named. Required.
# -v budget=N: exits 1, with a warning, when a screen dropped more than N%
#    of its frames. Without it, only reports.
# Exits 2 when nothing animated or a screen's refresh rate is missing.
BEGIN {
	LATE = 1.5
	BURST_GAP_MS = 100
	n = split(refresh, pairs, " ")
	for (i = 1; i <= n; i++) {
		split(pairs[i], pair, "=")
		hz_of[pair[1]] = pair[2] + 0
	}
}

match($0, /nova-frame: screen=[^ ]+ dt=[0-9]+/) {
	split(substr($0, RSTART + length("nova-frame: "), RLENGTH - length("nova-frame: ")), field, /[ =]/)
	screen = field[2]
	dt = field[4] + 0
	if (!(screen in frames)) {
		order[++screens] = screen
		frames[screen] = 0
		hz[screen] = screen in hz_of ? hz_of[screen] : hz_of["*"]
		if (!hz[screen]) {
			printf "frames: no refresh rate for screen %s\n", screen
			failed = 1
			exit 2
		}
	}
	if (dt > BURST_GAP_MS) {
		bursts[screen]++
		next
	}
	frames[screen]++
	if (dt > LATE * 1000 / hz[screen]) {
		dropped[screen]++
		missed = int(dt * hz[screen] / 1000 + 0.5) - 1
		missed_vblanks[screen] += missed > 1 ? missed : 1
	}
	if (dt > worst[screen])
		worst[screen] = dt
}

END {
	if (failed)
		exit 2
	if (!screens) {
		print "frames: no nova-frame lines; was the UI started with NOVA_FRAME_LOG=1?"
		exit 2
	}
	for (s in frames)
		animated += frames[s]
	if (!animated) {
		print "frames: nothing animated, only lone redraws"
		exit 2
	}
	printf "%-10s %8s %7s %7s %8s %8s %14s %8s\n", "screen", "hz", "bursts", "frames", "dropped", "dropped%", "missed vblanks", "worst ms"
	for (i = 1; i <= screens; i++) {
		s = order[i]
		if (!frames[s])
			continue
		pct = 100 * dropped[s] / frames[s]
		printf "%-10s %8.3f %7d %7d %8d %7.1f%% %14d %8d\n", s, hz[s], bursts[s], frames[s], dropped[s], pct, missed_vblanks[s], worst[s]
		if (budget != "" && pct > budget + 0)
			warnings = warnings sprintf("warning: %s dropped %.1f%% of its frames, more than the %s%% budget\n", s, pct, budget)
	}
	printf "%s", warnings
	exit warnings != ""
}
