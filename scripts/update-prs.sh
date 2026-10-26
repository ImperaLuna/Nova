#!/usr/bin/env bash
# Keeps open PRs mergeable without re-running CI on all of them each time
# main moves. .github/workflows/update-prs.yml runs it; it runs no PR code.
#
#   probe  Tries main's merge into every open PR in memory (git merge-tree)
#          and comments on each one that conflicts, once per PR head, naming
#          the files. Pushes nothing, so starts no CI.
#   lane   The merge lane, for armed PRs: those with GitHub's auto-merge on
#          and no `hold` label. Oldest armed first, it merges main into one
#          PR at a time; that push starts the PR's CI, and GitHub merges the
#          PR once `checks` passes on top of the current main (the ruleset
#          requires an up-to-date branch). While that PR's CI runs, the lane
#          waits. An armed PR whose `checks` failed, or that conflicts, is
#          skipped after one comment, until its owner pushes.
#
# Needs git 2.38+, gh and jq, a clone with origin's branches, and in the
# environment: GH_TOKEN (reads PRs, comments), TOKEN (pushes; a person's
# token, so the push starts CI that counts for the `checks` rule), BASE (the
# default branch) and GITHUB_REPOSITORY.
# Usage: scripts/update-prs.sh probe|lane
set -euo pipefail

mode=${1:?usage: update-prs.sh probe|lane}
# The label that keeps a PR out of the lane while auto-merge stays on.
HOLD=hold
# The job of ci.yml the ruleset requires.
CHECK=checks

auth=$(printf 'x-access-token:%s' "$TOKEN" | base64 -w0)
git() {
  command git -c core.hooksPath=/dev/null -c http.extraheader="AUTHORIZATION: basic $auth" \
    -c user.name='github-actions[bot]' \
    -c user.email='41898282+github-actions[bot]@users.noreply.github.com' "$@"
}
main=$(git rev-parse "refs/remotes/origin/$BASE")

# Comments once per marker: a PR nobody has pushed to since isn't told again.
comment() {
  local number=$1 marker=$2 text=$3
  if gh api --paginate "repos/$GITHUB_REPOSITORY/issues/$number/comments" --jq '.[].body' |
    grep -qF -- "$marker"; then
    echo "#$number was already told ($marker)."
    return
  fi
  gh pr comment "$number" --repo "$GITHUB_REPOSITORY" --body "$text

$marker"
}

# The files main's merge into `head` conflicts in, one per line; none when it
# merges cleanly.
conflicts() {
  local head=$1 out
  if out=$(git merge-tree --write-tree --name-only --no-messages "$main" "$head"); then
    return
  fi
  # The merged tree's id, then the conflicting files.
  tail -n +2 <<<"$out"
}

tell_conflict() {
  local number=$1 head=$2 files=$3
  echo "::warning::#$number conflicts with $BASE."
  # shellcheck disable=SC2016,SC2001 # literal backticks, for Markdown
  comment "$number" "<!-- update-prs head=$head -->" "\`$BASE\` moved to $main and no longer merges cleanly into this branch. Conflicting files:

$(sed 's/.*/- `&`/' <<<"$files")

Merge \`$BASE\` into the branch and resolve them."
}

# Open same-repo PRs into BASE, oldest armed first; fork branches aren't ours.
prs=$(gh pr list --repo "$GITHUB_REPOSITORY" --base "$BASE" --state open --limit 100 \
  --json number,headRefName,headRefOid,isCrossRepository,autoMergeRequest,labels,statusCheckRollup \
  --jq '[.[] | select(.isCrossRepository | not)]')
mapfile -t heads < <(jq -r '.[] | "+refs/heads/\(.headRefName):refs/remotes/origin/\(.headRefName)"' <<<"$prs")
if ((${#heads[@]})); then
  git fetch -q origin "${heads[@]}"
fi

probe() {
  local number head files
  while read -r number head; do
    if git merge-base --is-ancestor "$main" "$head"; then
      continue
    fi
    files=$(conflicts "$head")
    if [[ -n $files ]]; then
      tell_conflict "$number" "$head" "$files"
    else
      echo "#$number merges $BASE cleanly."
    fi
  done < <(jq -r '.[] | "\(.number) \(.headRefOid)"' <<<"$prs")
}

# The state of `checks` on a PR's head: pending (also before CI has started),
# success or failure.
check_state() {
  jq -r --arg check "$CHECK" '
    [.statusCheckRollup[]? | select(.name == $check)] | last
    | if . == null or .status != "COMPLETED" then "pending"
      elif .conclusion == "SUCCESS" then "success"
      else "failure" end'
}

lane() {
  local armed pr number branch head files out
  armed=$(jq -c --arg hold "$HOLD" '
    [.[] | select(.autoMergeRequest != null and ([.labels[].name] | index($hold) | not))]
    | sort_by(.autoMergeRequest.enabledAt) | .[]' <<<"$prs")
  if [[ -z $armed ]]; then
    echo "No armed PRs."
    return
  fi
  while read -r pr; do
    number=$(jq -r .number <<<"$pr")
    branch=$(jq -r .headRefName <<<"$pr")
    head=$(jq -r .headRefOid <<<"$pr")
    if git merge-base --is-ancestor "$main" "$head"; then
      case $(check_state <<<"$pr") in
        pending)
          echo "#$number is being tested on top of $BASE; the lane waits for it."
          return
          ;;
        success)
          echo "#$number passed on top of $BASE; GitHub merges it."
          return
          ;;
        failure)
          echo "::warning::#$number failed \`$CHECK\`; the lane skips it until a push."
          comment "$number" "<!-- merge-lane failed head=$head -->" "This PR is armed to merge, but \`$CHECK\` failed on top of \`$BASE\`, so the merge lane skips it. Push a fix; the lane picks it up again once it passes."
          continue
          ;;
      esac
    fi
    files=$(conflicts "$head")
    if [[ -n $files ]]; then
      tell_conflict "$number" "$head" "$files"
      continue
    fi
    git checkout -q --detach "$head"
    git merge -q --no-edit -m "Merge $BASE into $branch" "$main"
    local out
    if out=$(git push -q origin "HEAD:refs/heads/$branch" 2>&1); then
      echo "#$number ($branch): merged $BASE; its CI runs next."
      return
    fi
    # Not a fast-forward: its owner pushed meanwhile, which starts CI too.
    if [[ $(git ls-remote origin "refs/heads/$branch" | cut -f1) != "$head" ]]; then
      echo "::warning::#$number ($branch) moved during the update; the lane waits for its CI."
      return
    fi
    echo "::warning::#$number ($branch): push refused: $out"
    comment "$number" "<!-- update-prs push head=$head -->" "\`$BASE\` merges cleanly into this armed PR, but the merge lane couldn't push the merge. Merge \`$BASE\` into the branch by hand.

\`\`\`
$out
\`\`\`"
  done <<<"$armed"
  echo "No armed PR can go next."
}

case $mode in
  probe) probe ;;
  lane) lane ;;
  *)
    echo "usage: update-prs.sh probe|lane" >&2
    exit 2
    ;;
esac
