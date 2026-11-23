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
#   restack  From the bottom up, merges each branch's base into it, runs
#            $STACK_CHECK on the merge, and pushes (never forced), so every
#            branch holds the one below. Stops at the first conflict, naming
#            the files, or at the first branch whose check fails, and pushes
#            nothing from there up; the branch's own thread fixes it, then
#            restack runs again.
#   mark     For every open PR in a stack (its base isn't $BASE, or another
#            PR is built on it), sets a `stack` commit status on its head:
#            failure when the branch below has moved on without it, pending
#            when only $BASE has (restack once, just before merging, so CI
#            doesn't run for every move of $BASE), success when it holds its
#            base. So a stale stack shows on GitHub without any CI run. A PR
#            that left its stack gets its status cleared to success. Takes no
#            branches; needs gh. .github/workflows/stacks.yml runs it on
#            every push.
#
# A branch merges only its own base: merging one further down resolves the
# same conflict twice and ends in a rewrite (the Nova Files stack, IMP-326).
#
# Every branch is checked on its own, not only the top: a merge that breaks
# a lower branch (a lost brace, a stale snapshot) fails that PR's CI, and the
# stack can't merge (the Nova Files batch, IMP-351).
#
# Branches are named bottom to top; the base of the first is $BASE (main).
# Works in a temporary worktree, so the checkout it runs from is untouched;
# the check shares that checkout's cargo target dir. Run restack inside
# `nix develop`, where the check's tools are. Needs git; status's PR columns
# need a GitHub remote and gh, signed in, and are left out without.
# Usage: scripts/stack.sh status|restack [-n] <branch>... | mark
#   -n  restack without pushing: merge and check in the worktree and report.
set -euo pipefail

usage="usage: stack.sh status|restack [-n] <branch>... | mark"
mode=${1:?$usage}
shift
push=1
if [[ ${1:-} == -n ]]; then
  push=0
  shift
fi
if [[ $mode == mark ]]; then
  ((push && $# == 0)) || {
    echo "mark takes no branches and no -n" >&2
    exit 2
  }
elif (($# == 0)); then
  echo "$usage" >&2
  exit 2
fi
branches=("$@")
REMOTE=${REMOTE:-origin}
BASE=${BASE:-main}
# What restack runs on each merged branch before pushing it, from the
# branch's root; "none" skips it.
STACK_CHECK=${STACK_CHECK:-just check-branch}
# The commit status mark sets.
CONTEXT=stack
# The job of ci.yml the main ruleset requires.
CHECK=checks
# How long one fetch or push may take before the script gives up on it.
REMOTE_TIMEOUT=${REMOTE_TIMEOUT:-120}

# remote <git args>: a git command that talks to the remote, bounded.
remote() { timeout "$REMOTE_TIMEOUT" git "$@"; }

((${#branches[@]} == 0)) || remote fetch --quiet "$REMOTE" "$BASE" "${branches[@]}"

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
  local target
  target=${CARGO_TARGET_DIR:-$(git rev-parse --show-toplevel)/target}
  # The default check's tools come from the dev shell; without them every
  # branch would look broken.
  if [[ $STACK_CHECK == "just "* ]] && ! { command -v just >/dev/null && [[ -n ${IN_NIX_SHELL:-} ]]; }; then
    echo "restack checks each branch with \`$STACK_CHECK\`: run it inside \`nix develop\` (or set STACK_CHECK=none to skip the check)." >&2
    return 2
  fi
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
    if [[ $STACK_CHECK != none ]] && ! (cd "$dir" && CARGO_TARGET_DIR=$target bash -c "$STACK_CHECK") >&2; then
      echo "$branch: \`$STACK_CHECK\` fails after merging $(base_of "$i"); nothing pushed from here up." >&2
      echo "Its thread merges $REMOTE/$(base_of "$i"), fixes and pushes; then run restack again." >&2
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

# gh_api <args>: a GitHub API call, bounded.
gh_api() { timeout "$REMOTE_TIMEOUT" gh api "$@"; }

# set_status <repo> <sha> <state> <text>: the `stack` status on a commit,
# unless it already says that (GitHub keeps at most 1000 per commit).
set_status() {
  local now text=${4:0:140}
  now=$(gh_api "repos/$1/commits/$2/status" --jq ".statuses[] | select(.context == \"$CONTEXT\") | \"\(.state) \(.description)\"") || now=
  [[ $now == "$3 $text" ]] && return 0
  gh_api --method POST "repos/$1/statuses/$2" -f state="$3" -f context="$CONTEXT" -f description="$text" >/dev/null
}

mark() {
  local repo prs number branch head base stacked state text failed=0
  repo=${GITHUB_REPOSITORY:-$(timeout "$REMOTE_TIMEOUT" gh repo view --json nameWithOwner --jq .nameWithOwner)}
  # Same-repo open PRs, one "number branch head base stacked" per line.
  prs=$(gh_api --paginate "repos/$repo/pulls?state=open&per_page=100" --jq '.[]' | jq -rs --arg main "$BASE" --arg repo "$repo" '
    [.[] | select(.head.repo.full_name == $repo)] as $own
    | ($own | map(.base.ref)) as $bases
    | $own[]
    | (.base.ref != $main or (.head.ref as $h | $bases | index($h) != null)) as $stacked
    | "\(.number) \(.head.ref) \(.head.sha) \(.base.ref) \($stacked)"')
  if [[ -z $prs ]]; then
    echo "No open PRs."
    return 0
  fi
  while read -r number branch head base stacked; do
    if [[ $stacked != true ]]; then
      # Out of a stack now (its base merged, say): clear an old red mark.
      if gh_api "repos/$repo/commits/$head/status" --jq ".statuses[].context" | grep -x "$CONTEXT" >/dev/null; then
        set_status "$repo" "$head" success "Not in a stack" || failed=1
      fi
      continue
    fi
    # One ref at a time: a base deleted as its PR merged fails only its own.
    if ! remote fetch --quiet "$REMOTE" "$branch" "$base" 2>/dev/null ||
      [[ $(git rev-parse "$(ref "$branch")") != "$head" ]]; then
      # Pushed again or gone since the list; that change runs mark again.
      echo "#$number: $branch moved or its base is gone, skipped"
      continue
    fi
    if git merge-base --is-ancestor "$(ref "$base")" "$head"; then
      state=success text="Holds $base"
    elif [[ $base == "$BASE" ]]; then
      state=pending text="$BASE moved on: restack just before merging the stack"
    else
      state=failure text="Behind $base: run scripts/stack.sh restack on this stack"
    fi
    if set_status "$repo" "$head" "$state" "$text"; then
      echo "#$number $branch: $state ($text)"
    else
      echo "::warning::#$number $branch: couldn't set the $CONTEXT status"
      failed=1
    fi
  done <<<"$prs"
  return $failed
}

case $mode in
status) status ;;
restack) restack ;;
mark) mark ;;
*)
  echo "$usage" >&2
  exit 2
  ;;
esac
