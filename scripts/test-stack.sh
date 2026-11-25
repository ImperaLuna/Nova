#!/usr/bin/env bash
# Tests scripts/stack.sh on a three-branch stack in a throwaway repository
# with a local remote: status on a consistent stack, a branch behind its
# base, a branch carrying a lower branch its base lacks; restack's dry run,
# its merge and push, its stop at a conflict and at a failing check; and
# mark's statuses, against a stand-in for GitHub's API.
set -euo pipefail

script=$(realpath "$(dirname "$0")/stack.sh")
failed=0
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.com
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.com
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
# Restack's per-branch check, until the test of it below.
export STACK_CHECK=true

# expect <what> <expected> <actual>
expect() {
  if [[ $2 != "$3" ]]; then
    printf 'FAIL: %s\n  expected: %s\n  got:      %s\n' "$1" "$2" "$3" >&2
    failed=1
  fi
}

# commit <file> <line>: appends a line to a file and commits it.
commit() {
  echo "$2" >>"$1"
  git add "$1"
  git commit --quiet -m "$1: $2"
}

git init --quiet --bare "$work/remote.git"
git init --quiet -b main "$work/repo"
cd "$work/repo"
git remote add origin "$work/remote.git"
commit base.txt one
# a on main, b on a, c on b; each touches its own file.
git switch --quiet -c a && commit a.txt a1
git switch --quiet -c b && commit b.txt b1
git switch --quiet -c c && commit c.txt c1
git switch --quiet main
git push --quiet origin main a b c

# stack <args>: the script's output (stdout and stderr) and exit status.
stack() {
  local out status=0
  out=$("$script" "$@" 2>&1) || status=$?
  printf '%s\nexit %s' "$(cut -d' ' -f2,4- <<<"$out")" "$status"
}

expect "a consistent stack" \
  "a  holds main
b  holds a
c  holds b
exit 0" "$(stack status a b c)"

# a moves on: b is behind it, and so is c (it holds b, not new a).
git switch --quiet a && commit a.txt a2 && git push --quiet origin a
expect "b behind a" \
  "a  holds main
b  BEHIND a
c  holds b
exit 1" "$(stack status a b c)"

# c merges a directly, skipping b: it carries a's commit that b lacks.
git switch --quiet c && git merge --quiet --no-edit origin/a && git push --quiet origin c
expect "c carries a" \
  "a  holds main
b  BEHIND a
c  holds b  CARRIES commits of a that b lacks
exit 1" "$(stack status a b c)"

# Start c over without that merge (the test's own setup, not a stack rule).
git push --quiet --force origin "$(git rev-parse HEAD^1):refs/heads/c"

before=$(git ls-remote origin b c)
expect "dry run" \
  "merges a cleanly (not pushed)
merges b cleanly (not pushed)
exit 0" "$("$script" restack -n a b c 2>&1 | grep -v ': holds' | cut -d' ' -f2-; echo "exit ${PIPESTATUS[0]}")"
expect "dry run pushes nothing" "$before" "$(git ls-remote origin b c)"

expect "restack" "exit 0" "$("$script" restack a b c >/dev/null 2>&1; echo "exit $?")"
expect "consistent after restack" \
  "a  holds main
b  holds a
c  holds b
exit 0" "$(stack status a b c)"
expect "nothing was force-pushed" "0" \
  "$(git fetch --quiet origin && git merge-base --is-ancestor "$(cut -f1 <<<"$before" | tail -1)" origin/c; echo $?)"

# a and b change the same line: restack stops at b and pushes nothing above.
git switch --quiet a && git reset --quiet --hard origin/a && commit shared.txt from-a && git push --quiet origin a
git switch --quiet b && git reset --quiet --hard origin/b && commit shared.txt from-b && git push --quiet origin b
c_before=$(git rev-parse origin/c)
out=$("$script" restack a b c 2>&1) && status=0 || status=$?
expect "conflict exit" "1" "$status"
expect "conflict names the file" "  shared.txt" "$(grep '^  ' <<<"$out")"
git fetch --quiet origin
expect "conflict leaves c alone" "$c_before" "$(git rev-parse origin/c)"
expect "no worktree left behind" "1" "$(git worktree list | wc -l | tr -d ' ')"

# A check that fails on b: a merges cleanly into b, but b isn't pushed, nor c.
git switch --quiet a && git reset --quiet --hard origin/a && git reset --quiet --hard HEAD^ && git push --quiet --force origin a
git switch --quiet b && git reset --quiet --hard origin/b && git reset --quiet --hard HEAD^ && git push --quiet --force origin b
git switch --quiet a && commit a.txt a3 && git push --quiet origin a
before=$(git ls-remote origin b c)
# shellcheck disable=SC2016 # the check expands in restack, not here
out=$(STACK_CHECK='test "$(git log -1 --format=%s HEAD^2 2>/dev/null)" != "a.txt: a3"' "$script" restack a b c 2>&1) && status=0 || status=$?
expect "failing check exit" "1" "$status"
# shellcheck disable=SC2016 # literal backticks
expect "failing check names the branch" "1" "$(grep -c '^b: `.*` fails after merging a' <<<"$out")"
expect "failing check pushes nothing" "$before" "$(git ls-remote origin b c)"
expect "the default check needs the dev shell" "2" \
  "$(IN_NIX_SHELL='' STACK_CHECK='just check-branch' "$script" restack -n a b c >/dev/null 2>&1; echo $?)"
expect "a passing check pushes" "exit 0" "$(STACK_CHECK='test -f b.txt' "$script" restack a b c >/dev/null 2>&1; echo "exit $?")"

# mark: a stand-in gh answers the open PR list from $work/pulls.json, keeps
# each commit's latest status in $work/status-<sha>, and logs every status
# it's sent to $work/statuses.
mkdir "$work/bin"
# This bash, by path: the Nix sandbox has no /usr/bin/env.
{
  echo "#!$BASH"
  cat <<'GH'
case $* in
  "api --paginate repos/o/r/pulls?state=open&per_page=100 --jq .[]") jq -c '.[]' "$WORK/pulls.json" ;;
  "api repos/o/r/commits/"*"/status --jq .statuses[].context")
    sha=${2#repos/o/r/commits/} sha=${sha%/status}
    [[ ! -f $WORK/status-$sha ]] || echo stack ;;
  "api repos/o/r/commits/"*"/status --jq "*)
    sha=${2#repos/o/r/commits/} sha=${sha%/status}
    cat "$WORK/status-$sha" 2>/dev/null || true ;;
  "api --method POST repos/o/r/statuses/"*)
    echo "${4##*/} ${6#state=} ${10#description=}" >>"$WORK/statuses"
    echo "${6#state=} ${10#description=}" >"$WORK/status-${4##*/}" ;;
  *) echo "unexpected gh $*" >&2; exit 1 ;;
esac
GH
} >"$work/bin/gh"
chmod +x "$work/bin/gh"
# pull <number> <branch> <base> [repo]: one PR of the list.
pull() {
  printf '{"number":%s,"head":{"ref":"%s","sha":"%s","repo":{"full_name":"%s"}},"base":{"ref":"%s"}}\n' \
    "$1" "$2" "$(git rev-parse "origin/$2")" "${4:-o/r}" "$3"
}
mark() { WORK=$work PATH="$work/bin:$PATH" GITHUB_REPOSITORY=o/r "$script" mark >/dev/null; }
git fetch --quiet origin
git switch --quiet a && git reset --quiet --hard origin/a && commit a.txt a4 && git push --quiet origin a
git switch --quiet main && commit base.txt two && git push --quiet origin main
git switch --quiet -c solo main && commit solo.txt s1 && git push --quiet origin solo
git switch --quiet -c fork a && commit fork.txt f1 && git push --quiet origin fork
{ pull 1 a main; pull 2 b a; pull 3 c b; pull 4 solo main; pull 5 fork a other/r; } | jq -s . >"$work/pulls.json"
mark
a=$(git rev-parse origin/a) b=$(git rev-parse origin/b) c=$(git rev-parse origin/c)
expect "mark: a stack's PRs, not a solo PR nor a fork's" \
  "$a pending main moved on: restack just before merging the stack
$b failure Behind a: run scripts/stack.sh restack on this stack
$c success Holds b" "$(cat "$work/statuses")"
mark
expect "mark again: nothing new to say" "3" "$(wc -l <"$work/statuses" | tr -d ' ')"
# b merges and c moves onto main: neither a nor c is in a stack any more, so
# their marks clear.
{ pull 1 a main; pull 3 c main; } | jq -s . >"$work/pulls.json"
: >"$work/statuses"
echo "failure Behind b" >"$work/status-$c"
mark
expect "mark clears PRs that left their stack" "$a success Not in a stack
$c success Not in a stack" "$(cat "$work/statuses")"
expect "mark: no branches" "2" "$("$script" mark a >/dev/null 2>&1; echo $?)"

if ((failed)); then exit 1; fi
echo "stack.sh: all tests passed"
