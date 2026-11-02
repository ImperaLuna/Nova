# shellcheck shell=bash disable=SC2034  # variables for the scripts that source it
# Shared by the scripts in scripts/desktop (sourced, not run). They help
# local sessions on the maintainer's desktop (RyzenShine) with the steps
# every test there repeats; AGENTS.md says when to use which.

# GitHub's name for Nova, and the main checkout every worktree hangs off
# (the one these scripts live in, or the one a worktree's .git points to).
github_repo=ImperaLuna/Nova
main_checkout=$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --path-format=absolute --git-common-dir)
main_checkout=${main_checkout%/.git}
worktrees=$main_checkout/.claude/worktrees

die() {
  echo "$(basename "$0"): $*" >&2
  exit 1
}

# Fetches `branch` into origin/<branch> over HTTPS with gh's login. Not
# over SSH: this desktop's ssh refuses its system config ("Bad owner or
# permissions"), and the timer that keeps ~/Projects/Nova current fetches
# the same way. Pushing isn't affected; local sessions don't push.
fetch_branch() { # <branch>
  git -C "$main_checkout" -c credential.helper= -c credential.helper='!gh auth git-credential' \
    fetch --quiet "https://github.com/$github_repo.git" "+refs/heads/$1:refs/remotes/origin/$1" ||
    die "can't fetch $1 from GitHub (is \`gh auth status\` logged in?)"
}

# Runs a command inside Nova's dev shell (cargo, just...), entering it with
# `nix develop` unless this shell already is one.
in_dev_shell() { # <checkout> <command...>
  local checkout=$1
  shift
  if [[ -n ${IN_NIX_SHELL:-} ]]; then
    (cd "$checkout" && "$@")
  else
    (cd "$checkout" && nix develop --command "$@")
  fi
}
