#!/usr/bin/env bash
# Lists every VM subtest that failed in CI in one tracking issue, so a flaky
# one shows as a repeat without anyone searching old runs.
# .github/workflows/vm-failures.yml runs `record` after each failed CI run;
# it runs no PR code.
#
#   parse [log...]  The failed VM subtests in CI job logs (or stdin), one
#                   JSON object per line: {vm, subtest, error}. vm is the
#                   VM test's half, as niri-frames or hyprland-session. A VM
#                   test that failed outside any subtest (booting, logging
#                   in) is listed with OUTSIDE as its subtest.
#   render [data]   The tracking issue's body for the records in `data` (or
#                   stdin), one JSON object per line: {vm, subtest, error,
#                   pr, run, date}. Each subtest gets one row, newest first;
#                   one that failed on more than one PR is marked a repeat.
#                   The records go at the end, in a comment, for the next
#                   `record`.
#   records [body]  The records in an issue body that `render` wrote (or
#                   stdin), one per line.
#   record          Reads the failed jobs' logs of one CI run, adds what
#                   `parse` finds to the tracking issue (labelled LABEL;
#                   made if there's none), and comments on the PR when a
#                   subtest that failed there failed on another PR before.
#
# `record` needs gh and jq, and in the environment: GH_TOKEN (reads Actions
# logs, writes issues and PR comments), GITHUB_REPOSITORY, RUN_ID and
# RUN_ATTEMPT (the CI run), PR (its pull request's number; empty for none)
# and RUN_DATE (when it ran, ISO 8601).
# Usage: scripts/vm-failures.sh parse|render|records|record [file...]
set -euo pipefail

mode=${1:?usage: vm-failures.sh parse|render|records|record [file...]}
shift
# The tracking issue's label and title.
LABEL=vm-failures
TITLE="VM test failures"
# What a failure outside any subtest is listed as.
OUTSIDE="(outside any subtest)"
# Where the records start and end in the issue's body.
DATA_START="<!-- vm-failures-data"
DATA_END="-->"
# How many records the issue keeps (its body holds at most 65536
# characters), and how many runs a row links.
KEPT_RECORDS=200
LINKED_RUNS=5
# A subtest's error, cut to this many characters in its row.
ERROR_CHARS=160

parse() {
  # The driver logs `!!! Test "<name>" failed with error: "<error>"` under
  # the VM's prefix. Nix then names each VM test whose builder failed, with
  # its last log lines: the same line again, or for a failure outside any
  # subtest, its error.
  jq -cRn --arg outside "$OUTSIDE" '
    def failed_test: capture("!!! Test \"(?<subtest>.*)\" failed with error: \"(?<error>.*)\"$");
    [inputs | sub("\r$"; "")] as $lines
    | [$lines[] | capture("vm-test-run-nova-(?<vm>[a-z]+-[a-z]+)> (?<line>!!! Test .*)$") | {vm} + (.line | failed_test)]
    | . as $logged
    | [range(0; $lines | length) as $i
        | $lines[$i] | capture("Cannot build \\x27[^\\x27]*-vm-test-run-nova-(?<vm>[a-z]+-[a-z]+)\\.drv\\x27")
        | select(($lines[$i + 1] // "") | test("builder failed"))
        | .vm as $vm
        | $lines[$i + 2:$i + 40] as $last
        | ([$last[] | select(test("^\\s*> !!! Test ")) | failed_test] | last)
            // {subtest: $outside, error: ([$last[] | capture("> [A-Za-z_.]*(Error|Failed): (?<e>.*)$").e] | last // "")}
        | {vm: $vm} + .] as $built
    | ($logged + $built | unique_by([.vm, .subtest])) as $all
    # A VM whose failed subtest is known did not also fail outside one.
    | [$all[] | select(.subtest != $outside) | .vm] as $named
    | $all[] | select(.subtest != $outside or (.vm as $vm | $named | index($vm) | not))' "$@"
}

render() {
  jq -rs --arg start "$DATA_START" --arg end "$DATA_END" --argjson runs "$LINKED_RUNS" \
    --argjson chars "$ERROR_CHARS" '
    def cell: tostring | gsub("\\|"; "\\|") | gsub("\n"; " ");
    def short: if length > $chars then .[:$chars] + "…" else . end;
    def prs: map(.pr | select(. != null and . != "")) | unique;
    (group_by([.vm, .subtest])
      | map(sort_by(.date) | {last: .[-1], count: length, prs: prs, runs: .[-$runs:]})
      | sort_by(.last.date) | reverse) as $rows
    | "Every VM subtest that failed in CI, newest first, kept by `scripts/vm-failures.sh` after each failed CI run. A subtest that failed on more than one PR (**repeat**) is flaky or broken on main: start a fix ticket for it instead of re-running CI.",
      "",
      "| Subtest | VM | Failures | PRs | Last failed | Last error | Runs |",
      "|---|---|---|---|---|---|---|",
      ($rows[]
        | "| \(if (.prs | length) > 1 then "**repeat** " else "" end)\(.last.subtest | cell) | \(.last.vm) | \(.count) | \(.prs | map("#\(.)") | join(", ")) | \(.last.date[:10]) | \(.last.error | short | cell) | \([.runs | to_entries[] | "[\(.key + 1)](\(.value.run))"] | join(" ")) |"),
      "",
      $start,
      # Escaped, so an error with `-->` in it cannot end the comment early.
      (.[] | tojson | gsub("-->"; "--\\u003e")),
      $end' "$@"
}

records() {
  tr -d '\r' <"${1:-/dev/stdin}" | awk -v start="$DATA_START" -v end="$DATA_END" '
    $0 == start { inside = 1; next }
    inside && $0 == end { exit }
    inside'
}

record() {
  : "${GITHUB_REPOSITORY:?}" "${RUN_ID:?}" "${RUN_ATTEMPT:?}" "${RUN_DATE:?}" "${PR?}"
  local repo=$GITHUB_REPOSITORY work
  local run="https://github.com/$repo/actions/runs/$RUN_ID/attempts/$RUN_ATTEMPT"
  work=$(mktemp -d)
  trap 'rm -rf "$work"' EXIT

  local job
  for job in $(gh api "repos/$repo/actions/runs/$RUN_ID/attempts/$RUN_ATTEMPT/jobs" \
    --jq '.jobs[] | select(.conclusion == "failure") | .id'); do
    gh api "repos/$repo/actions/jobs/$job/logs" >"$work/job-$job.log"
  done
  if ! compgen -G "$work/job-*.log" >/dev/null; then
    echo "No job of $run failed."
    return
  fi
  parse "$work"/job-*.log |
    jq -c --arg pr "$PR" --arg run "$run" --arg date "$RUN_DATE" '. + {pr: $pr, run: $run, date: $date}' \
      >"$work/new"
  if [[ ! -s $work/new ]]; then
    echo "No VM test failed in $run."
    return
  fi
  echo "Failed in $run:"
  jq -r '"  \(.vm): \(.subtest)"' "$work/new"

  local issue
  issue=$(gh issue list --repo "$repo" --label "$LABEL" --state open --json number --jq '.[0].number // empty')
  if [[ -z $issue ]]; then
    gh label create "$LABEL" --repo "$repo" --force --color d93f0b \
      --description "The VM subtests that failed in CI (scripts/vm-failures.sh)"
    issue=$(gh issue create --repo "$repo" --label "$LABEL" --title "$TITLE" --body "Filled in by the next failed CI run." |
      grep -o '[0-9]*$')
  fi
  gh issue view "$issue" --repo "$repo" --json body --jq .body | records >"$work/old"

  # A re-delivered run adds nothing twice.
  cat "$work/old" "$work/new" | jq -cs --argjson kept "$KEPT_RECORDS" \
    'unique_by([.run, .vm, .subtest]) | sort_by(.date) | .[-$kept:][]' >"$work/all"
  render "$work/all" >"$work/body"
  gh issue edit "$issue" --repo "$repo" --body-file "$work/body"
  echo "Recorded in https://github.com/$repo/issues/$issue"

  [[ -n $PR ]] || return 0
  # Each subtest that failed here and before on another PR, with those PRs.
  jq -rs --slurpfile old "$work/old" --arg pr "$PR" --arg outside "$OUTSIDE" '
    .[] | select(.subtest != $outside) | . as $failure
    | [$old[] | select(.vm == $failure.vm and .subtest == $failure.subtest and .pr != $pr and .pr != "") | .pr]
    | unique | select(length > 0)
    | "- `\($failure.subtest)` (\($failure.vm)), before on \(map("#\(.)") | join(", "))"' "$work/new" >"$work/repeats"
  if [[ -s $work/repeats ]]; then
    gh pr comment "$PR" --repo "$repo" --body "These VM subtests failed in [this run]($run), and on other PRs before ([tracking issue](https://github.com/$repo/issues/$issue)):

$(cat "$work/repeats")

A second failure on an unrelated PR means the subtest is flaky or broken on main: start a fix ticket for it rather than re-running CI."
  fi
}

case $mode in
parse) parse "$@" ;;
render) render "$@" ;;
records) records "$@" ;;
record) record ;;
*)
  echo "usage: vm-failures.sh parse|render|records|record [file...]" >&2
  exit 2
  ;;
esac
