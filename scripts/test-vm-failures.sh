#!/usr/bin/env bash
# Tests scripts/vm-failures.sh's parse, render and records on CI logs and
# failures recorded from real runs (scripts/vm-failures/). `record` talks to
# GitHub, so only its parts are tested here.
set -euo pipefail

cd "$(dirname "$0")"
script=./vm-failures.sh
data=vm-failures
failed=0

# expect <what> <expected> <actual>
expect() {
  if [[ $2 != "$3" ]]; then
    printf 'FAIL: %s\n  expected: %s\n  got:      %s\n' "$1" "$2" "$3" >&2
    failed=1
  fi
}

# One failed subtest, as the driver logs it under the VM's prefix.
expect "one failed subtest" \
  '{"vm":"niri-frames","subtest":"a playing pill draws only its equalizer'\''s steps; a paused one draws nothing","error":"a playing pill made 54 commits in 5 s"}' \
  "$($script parse "$data/one-subtest.log")"

# Two VMs: hyprland-session with its subtest's line; niri-frames whose line
# the log doesn't hold, so only Nix's last lines name its error.
expect "two VMs, one without its subtest's line" \
  'hyprland-session	nova toggle launcher opens the launcher, empty; typing finds foot, Enter starts it in a scope of its own, Esc closes	action timed out after 120.25 seconds (timeout=120.0)
niri-frames	(outside any subtest)	3 clicks at (24, 22) never logged '\''nova: ai usage panel opened'\''' \
  "$($script parse "$data/two-vms.log" | jq -r '[.vm, .subtest, .error] | @tsv')"

# A failed run whose VM tests all passed (clippy failed) lists nothing.
expect "no VM test failed" "" "$($script parse "$data/no-vm-test.log")"

# The same log on stdin.
expect "parse reads stdin" "$($script parse "$data/one-subtest.log")" "$($script parse <"$data/one-subtest.log")"

body=$($script render "$data/records.jsonl")
rows=$(grep '^| ' <<<"$body" | tail -n +2)

# One row per subtest, newest first; the PRs it failed on, each once.
expect "rows by subtest, newest first" \
  "a playing pill draws only its equalizer's steps; a paused one draws nothing
an idle bar draws nothing, shown or beside a fullscreen window; the panel draws briefly; the launcher over a fullscreen window moves Nova to Overlay and back, each time; the bar gives a fullscreen window its space, and its pills take clicks once back
nova toggle launcher opens the launcher, empty; typing finds foot, Enter starts it in a scope of its own, Esc closes
nova toggle power opens the power menu; Esc closes it and its L key locks" \
  "$(grep -v '^|---' <<<"$rows" | cut -d'|' -f2 | sed 's/^ \(\*\*repeat\*\* \)\{0,1\}//; s/ $//')"
expect "counts and PRs" \
  " 4 | #129, #131, #138 " \
  "$(grep -F 'a playing pill' <<<"$rows" | cut -d'|' -f4,5)"

# A repeat is a subtest that failed on more than one PR; two runs of one PR
# aren't.
expect "repeats" "3" "$(grep -c '^| \*\*repeat\*\* ' <<<"$rows")"
expect "one PR is no repeat" "1" "$(grep -c '^| nova toggle power' <<<"$rows")"
# Failures outside any subtest share a row per VM, but not a cause.
expect "outside a subtest is no repeat" "0" \
  "$(jq -c '.subtest = "(outside any subtest)"' "$data/records.jsonl" | $script render | grep -c '^| \*\*repeat\*\* ' || true)"

# A row links its latest runs.
expect "linked runs" "5" "$(grep -F 'an idle bar' <<<"$rows" | grep -o '](https://' | wc -l | tr -d ' ')"

# A `|` in an error doesn't end the table's cell.
expect "pipes escaped" '| a \| b |' \
  "$(jq -c '.error = "a | b"' "$data/records.jsonl" | $script render | grep -o '| a \\| b |' | head -n1)"

# An error with `-->` in it stays inside the records' comment.
expect "comment end escaped" "a --> b" \
  "$(jq -c '.error = "a --> b"' "$data/records.jsonl" | $script render | $script records | jq -r .error | sort -u)"

# What render wrote, records reads back.
expect "records round trip" "$(jq -c . "$data/records.jsonl")" "$($script records <<<"$body")"
expect "records with CRLF" "$(jq -c . "$data/records.jsonl")" "$($script records <<<"${body//$'\n'/$'\r\n'}")"
expect "no records" "" "$($script records <<<"Filled in by the next failed CI run.")"

if [[ $failed == 0 ]]; then
  echo "vm-failures.sh: all tests passed"
fi
exit "$failed"
