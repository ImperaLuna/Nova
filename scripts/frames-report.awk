# Missed vblanks per surface, from the UI's `nova-frame:` lines
# (ui/stage/FrameLog.qml, NOVA_FRAME_LOG=1) in any log on stdin or in the
# files named. `just frames` runs it on a live desktop; the VM tests print it.
#
# A frame that comes more than LATE vblanks after the one before missed a
# vblank: the screen showed the old frame again. The log splits into bursts
# at each `nova-frames: toggle` line (MARK, which `just frames` writes before
# each toggle); without one, the whole log is one burst. A screen's first
# frame in a burst ends idle time, so its gap isn't frame time.
#
# A screen's animation in a burst starts with its first frame on time.
# From there every late gap counts, however long, so a stall inside an
# animation still shows. Late frames before it are lone redraws: the strip
# redrawing once at the toggle and again when a panel elsewhere closes back
# into its pill, a screen redrawing while another one animates, or a stage a
# fullscreen window hides, which the compositor then gives a frame only now
# and then. Those gaps aren't stutter and aren't counted; the longest one
# before an animation shows as "start ms", so a stall right as an animation
# begins can still be seen. A burst with no frame on time is all lone
# redraws and shows in the "idle" column, and a screen that never animated
# is listed apart as idle or hidden.

# A surface is a screen's stage ("DP-1") or one of its PanelSurfaces
# ("DP-1/right"), each reported on its own row; "screen" below means either.
#
# -v refresh="DP-1=144 DP-2=120": each screen's refresh rate in Hz; `*=60`
#    stands for every screen not named. Required.
# -v budget=N: exits 1, with a warning, when a screen dropped more than N%
#    of its frames. Without it, only reports.
# Exits 2 when nothing animated or a screen's refresh rate is missing.
BEGIN {
	LATE = 1.5
	MARK = "nova-frames: toggle"
	n = split(refresh, pairs, " ")
	for (i = 1; i <= n; i++) {
		split(pairs[i], pair, "=")
		hz_of[pair[1]] = pair[2] + 0
	}
}

index($0, MARK) {
	marks++
	next
}

match($0, /nova-frame: screen=[^ ]+ dt=[0-9]+/) {
	split(substr($0, RSTART + length("nova-frame: "), RLENGTH - length("nova-frame: ")), field, /[ =]/)
	screen = field[2]
	dt = field[4] + 0
	if (!(screen in frames)) {
		order[++screens] = screen
		frames[screen] = 0
		# A PanelSurface runs at its screen's rate.
		output = screen
		sub(/\/.*/, "", output)
		hz[screen] = output in hz_of ? hz_of[output] : hz_of["*"]
		if (!hz[screen]) {
			printf "frames: no refresh rate for screen %s\n", screen
			failed = 1
			exit 2
		}
	}
	if (!(screen in seen_mark) || seen_mark[screen] != marks) {
		close_burst(screen)
		seen_mark[screen] = marks
		next
	}
	if (dt <= LATE * 1000 / hz[screen]) {
		burst_on_time[screen]++
	} else if (!burst_on_time[screen]) {
		# Not animating yet: a lone redraw.
		if (dt > burst_start[screen])
			burst_start[screen] = dt
		next
	} else {
		burst_dropped[screen]++
		missed = int(dt * hz[screen] / 1000 + 0.5) - 1
		burst_missed[screen] += missed > 1 ? missed : 1
	}
	burst_frames[screen]++
	if (dt > burst_worst[screen])
		burst_worst[screen] = dt
}

# Adds a screen's current burst to its totals when it animated, or counts it
# as idle when it was only lone redraws, and starts the next one.
function close_burst(s) {
	if (!(s in seen_mark))
		return
	if (burst_on_time[s]) {
		bursts[s]++
		frames[s] += burst_frames[s]
		dropped[s] += burst_dropped[s]
		missed_vblanks[s] += burst_missed[s]
		if (burst_worst[s] > worst[s])
			worst[s] = burst_worst[s]
		if (burst_start[s] > start[s])
			start[s] = burst_start[s]
	} else {
		idle[s]++
	}
	burst_frames[s] = burst_on_time[s] = burst_dropped[s] = burst_missed[s] = burst_worst[s] = burst_start[s] = 0
}

END {
	if (failed)
		exit 2
	for (i = 1; i <= screens; i++)
		close_burst(order[i])
	if (!screens) {
		print "frames: no nova-frame lines; was the UI started with NOVA_FRAME_LOG=1?"
		exit 2
	}
	for (s in frames)
		animated += frames[s]
	if (!animated) {
		print "frames: nothing animated, only lone redraws or hidden screens"
		exit 2
	}
	printf "%-14s %8s %7s %5s %7s %8s %8s %14s %8s %8s\n", "surface", "hz", "bursts", "idle", "frames", "dropped", "dropped%", "missed vblanks", "worst ms", "start ms"
	for (i = 1; i <= screens; i++) {
		s = order[i]
		if (!frames[s]) {
			if (idle[s])
				lone = lone sprintf(" %s (%d)", s, idle[s])
			continue
		}
		pct = 100 * dropped[s] / frames[s]
		printf "%-14s %8.3f %7d %5d %7d %8d %7.1f%% %14d %8d %8d\n", s, hz[s], bursts[s], idle[s], frames[s], dropped[s], pct, missed_vblanks[s], worst[s], start[s]
		if (budget != "" && pct > budget + 0)
			warnings = warnings sprintf("warning: %s dropped %.1f%% of its frames, more than the %s%% budget\n", s, pct, budget)
	}
	if (lone != "")
		printf "idle or hidden, only lone redraws (bursts):%s\n", lone
	printf "%s", warnings
	exit warnings != ""
}
