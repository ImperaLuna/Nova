#!/usr/bin/env bash
# Keeps a stack of PRs (one chain: the bottom PR targets main, each PR above
# targets the branch of the one below) consistent while many threads work on
# it, so one merge of the top PR lands them all.
#
#   status   One line per branch, bottom to top: whether it holds its base,
#            whether it carries commits from a branch further down (or main)
#            that its base lacks, and with gh: its PR, the PR's base, whether
#            it conflicts (GitHub then runs no CI for it) and its `checks`
#            on the current head. Exits 1 when anything needs doing.
#   restack  From the bottom up, merges each branch's base into it and
#            pushes (never forced), so every branch holds the one below.
#            Stops at the first conflict and names the files; the branch's
#            own thread resolves it, then restack runs again.
#
# A branch merges only its own base: merging one further down resolves the
# same conflict twice and ends in a rewrite (the Nova Files stack, IMP-326).
#
# Branches are named bottom to top; the base of the first is $BASE (main).
# Works in a temporary worktree, so the checkout it runs from is untouched.
# Needs git; status's PR columns need a GitHub remote and gh, signed in, and
# are left out without.
# Usage: scripts/stack.sh status|restack [-n] <branch>...
#   -n  restack without pushing: merge in the worktree and report.
set -euo pipefail

usage="usage: stack.sh status|restack [-n] <branch>..."
mode=${1:?$usage}
shift
push=1
if [[ ${1:-} == -n ]]; then
  push=0
  shift
fi
(($# > 0)) || {
  echo "$usage" >&2
  exit 2
}
branches=("$@")
REMOTE=${REMOTE:-origin}
BASE=${BASE:-main}
# The job of ci.yml the main ruleset requires.
CHECK=checks
# How long one fetch or push may take before the script gives up on it.
REMOTE_TIMEOUT=${REMOTE_TIMEOUT:-120}

# remote <git args>: a git command that talks to the remote, bounded.
remote() { timeout "$REMOTE_TIMEOUT" git "$@"; }

remote fetch --quiet "$REMOTE" "$BASE" "${branches[@]}"

# ref <name>: the remote-tracking ref of a branch.
ref() { echo "refs/remotes/$REMOTE/$1"; }

# base_of <index>: the branch the one at <index> is built on.
base_of() {
  if (($1 == 0)); then echo "$BASE"; else echo "${branches[$1 - 1]}"; fi
}

# foreign <index>: the branches further down (and main) whose commits the
# branch carries but its base doesn't, space-separated.
foreign() {
  local i=$1 branch=${branches[$1]} base below found=()
  base=$(base_of "$i")
  for below in "$BASE" "${branches[@]:0:i}"; do
    [[ $below == "$base" ]] && continue
    if [[ -n $(comm -12 \
      <(git rev-list "$(ref "$below")" "^$(ref "$base")" | sort) \
      <(git rev-list "$(ref "$branch")" "^$(ref "$base")" | sort)) ]]; then
      found+=("$below")
    fi
  done
  echo "${found[*]}"
}

# pr_line <branch> <expected base>: the PR columns, from gh; empty without.
# A line starting with "!" needs doing; status drops the mark.
pr_line() {
  [[ $(git remote get-url "$REMOTE") == *github.com* ]] && command -v gh >/dev/null && timeout "$REMOTE_TIMEOUT" gh auth status >/dev/null 2>&1 || return 0
  local pr
  pr=$(timeout "$REMOTE_TIMEOUT" gh pr view "$1" --json number,baseRefName,mergeable,headRefOid,statusCheckRollup 2>/dev/null) || {
    echo "!  no open PR"
    return 0
  }
  # A check still queued or running needs nothing yet; any other result but
  # success (failure, cancelled, timed out...) does.
  jq -r --arg base "$2" --arg check "$CHECK" '
    ([.statusCheckRollup[] | select(.name == $check)] | last) as $c
    | ($c.conclusion // "" | ascii_downcase) as $done
    | (.baseRefName != $base or .mergeable == "CONFLICTING" or $c == null
       or ($done != "" and $done != "success")) as $todo
    | (if $todo then "!" else "" end)
      + "  #\(.number)"
      + (if .baseRefName == $base then "" else "  PR base is \(.baseRefName), not \($base)" end)
      + (if .mergeable == "CONFLICTING" then "  conflicts with its base: no CI runs" else "" end)
      + "  \($check): " + (if $c == null then "no run for \(.headRefOid[0:7])"
                           elif $done != "" then $done else ($c.status | ascii_downcase) end)
  ' <<<"$pr"
}

status() {
  local i branch base problems=0 line carries cols
  for i in "${!branches[@]}"; do
    branch=${branches[$i]}
    base=$(base_of "$i")
    line="$((i + 1)) $branch $(git rev-parse --short "$(ref "$branch")")"
    if git merge-base --is-ancestor "$(ref "$base")" "$(ref "$branch")"; then
      line+="  holds $base"
    else
      line+="  BEHIND $base"
      problems=1
    fi
    carries=$(foreign "$i")
    if [[ -n $carries ]]; then
      line+="  CARRIES commits of $carries that $base lacks"
      problems=1
    fi
    cols=$(pr_line "$branch" "$base")
    if [[ $cols == "!"* ]]; then
      problems=1
      cols=${cols#!}
    fi
    echo "$line$cols"
  done
  return $problems
}

restack() {
  local i branch base
  # Each branch's tip once restacked, so the branch above merges that.
  local -A tip=()
  dir=$(mktemp -d)
  git worktree add --quiet --detach "$dir" "$(ref "${branches[0]}")"
  trap 'git worktree remove --force "$dir"' EXIT
  for i in "${!branches[@]}"; do
    branch=${branches[$i]}
    base=$(base_of "$i")
    base=${tip[$base]:-$(git rev-parse "$(ref "$base")")}
    git -C "$dir" checkout --quiet --detach "$(ref "$branch")"
    if git -C "$dir" merge-base --is-ancestor "$base" HEAD; then
      echo "$branch: holds $(base_of "$i")"
      tip[$branch]=$(git -C "$dir" rev-parse HEAD)
      continue
    fi
    if ! git -C "$dir" merge --quiet --no-edit -m "Merge $(base_of "$i") into $branch" "$base" >/dev/null 2>&1; then
      echo "$branch: merging $(base_of "$i") conflicts in:" >&2
      git -C "$dir" diff --name-only --diff-filter=U | sed 's/^/  /' >&2
      git -C "$dir" merge --abort
      echo "Its thread merges $REMOTE/$(base_of "$i"), resolves and pushes; then run restack again." >&2
      return 1
    fi
    tip[$branch]=$(git -C "$dir" rev-parse HEAD)
    if ((push)); then
      remote -C "$dir" push --quiet "$REMOTE" "HEAD:refs/heads/$branch"
      echo "$branch: merged $(base_of "$i"), pushed $(git -C "$dir" rev-parse --short HEAD)"
    else
      echo "$branch: merges $(base_of "$i") cleanly (not pushed)"
    fi
  done
}

case $mode in
status) status ;;
restack) restack ;;
*)
  echo "$usage" >&2
  exit 2
  ;;
esac
