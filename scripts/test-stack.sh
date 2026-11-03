#!/usr/bin/env bash
# Tests scripts/stack.sh on a three-branch stack in a throwaway repository
# with a local remote: status on a consistent stack, a branch behind its
# base, a branch carrying a lower branch its base lacks; restack's dry run,
# its merge and push, and its stop at a conflict.
set -euo pipefail

script=$(realpath "$(dirname "$0")/stack.sh")
failed=0
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.com
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.com
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1

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

if ((failed)); then exit 1; fi
echo "stack.sh: all tests passed"
