#!/usr/bin/env bash
# Fails when a crate sits outside Nova's layout or depends on a crate it may
# not use, naming the broken rule and what to do instead. The rules and the
# folder kinds are in AGENTS.md ("Layout"); the rule ids printed here are the
# ones in its table.
#
# Two checks:
#   - The layout: a crate's folder decides what it may use (layer_of below),
#     so a new crate can't opt out. crates/ is the old layout, emptied by the
#     novad move (IMP-368); its crates may use anything, but nothing in the
#     new folders may use them.
#   - Allowed lists: each Cargo.toml lists the outside and workspace crates it
#     may use under its "# Allowed outside crates:" and "# Allowed workspace
#     crates:" lines, one "# - name" line each, or "none" on the header line,
#     so adding a dependency is a visible line in a PR.
# Dev-dependencies are not checked. Before reading the workspace, the script
# tests its own rules on known bad cases (expect_rule at the bottom of the
# rules), so a broken rule can't pass silently.
#
# Usage: scripts/check-layout.sh [workspace root]
# Needs cargo and jq; reads no network (cargo metadata --no-deps --offline).
set -euo pipefail

root=$(realpath "${1:-.}")
# Where the rules (and their fixes) are written down, for the messages.
rules_doc="AGENTS.md (Layout)"

# The framework crates each kind of crate may use, by their folder under
# framework/. A framework crate goes on a list only when every crate of that
# kind may use it: a daemon-side part (the socket server) never on the ui
# list, a screen-side part (the stage, theme tokens) never on the core list.
# Empty until the novad move adds the first framework crates (IMP-371).
FRAMEWORK_FOR_API=()
FRAMEWORK_FOR_CORE=()
FRAMEWORK_FOR_UI=()
FRAMEWORK_FOR_FRONT=()

# layer_of <crate folder relative to the root>: prints the crate's kind and
# its parts ("feature <name> <half>", "framework <name>", ...).
layer_of() {
  # A case pattern's * also matches /, so each kind is matched with its
  # exact depth: framework/a/b is outside the layout, not "framework a/b".
  local -a parts
  IFS=/ read -r -a parts <<<"$1"
  case ${#parts[@]}:${parts[0]} in
    1:app) echo app ;;
    2:crates) echo old ;;
    2:fronts) echo front ;;
    2:framework) echo "framework ${parts[1]}" ;;
    3:features)
      case ${parts[2]} in
        api | core | ui) echo "feature ${parts[1]} ${parts[2]}" ;;
        *) echo "unknown $1" ;;
      esac
      ;;
    *) echo "unknown $1" ;;
  esac
}

# in_list <word> <words...>
in_list() {
  local word=$1 each
  shift
  for each in "$@"; do
    [[ $each == "$word" ]] && return 0
  done
  return 1
}

# framework_names <names...>: "framework/{a,b}", or "no framework crate"
# while the list is empty.
framework_names() {
  if (($# == 0)); then
    echo "no framework crate"
  else
    local IFS=,
    echo "framework/{$*}"
  fi
}

# rule_break <from folder> <to folder>: prints the rule a dependency from the
# first crate on the second breaks, if any.
rule_break() {
  local -a from to
  read -r -a from <<<"$(layer_of "$1")"
  read -r -a to <<<"$(layer_of "$2")"
  if [[ ${from[0]} == unknown || ${to[0]} == unknown ]]; then
    echo "rule C1: every crate sits in app/, framework/<name>/, features/<name>/{api,core,ui}/, fronts/<name>/ or the old crates/."
    return
  fi
  case ${from[0]} in
    app | old) return ;;
  esac
  if [[ ${to[0]} == old ]]; then
    echo "rule M1: only crates/ and app/ use a crate still in crates/. Move what it needs into framework/ (or its feature) first."
    return
  fi
  case ${from[0]} in
    framework)
      [[ ${to[0]} == framework ]] && return
      echo "rule W1: the framework never uses a feature, a front or the app. Have the feature plug in instead (a module, a port or a message type)."
      ;;
    front)
      if [[ ${to[0]} == framework ]]; then
        in_list "${to[1]}" "${FRAMEWORK_FOR_FRONT[@]}" && return
        echo "rule W2: a front may use $(framework_names "${FRAMEWORK_FOR_FRONT[@]}") from the framework (the list in scripts/check-layout.sh)."
        return
      fi
      [[ ${to[0]} == feature && ${to[2]} == api ]] && return
      echo "rule W2: a front uses only listed framework crates and feature api crates. Send a message instead."
      ;;
    feature)
      local half=${from[2]}
      case ${to[0]} in
        framework)
          local -a framework_ok
          case $half in
            api) framework_ok=("${FRAMEWORK_FOR_API[@]}") ;;
            core) framework_ok=("${FRAMEWORK_FOR_CORE[@]}") ;;
            ui) framework_ok=("${FRAMEWORK_FOR_UI[@]}") ;;
          esac
          in_list "${to[1]}" "${framework_ok[@]}" && return
          local rule
          case $half in
            api) rule=F1 ;;
            core) rule=F2 ;;
            ui) rule=F3 ;;
          esac
          echo "rule $rule: a feature's $half may use $(framework_names "${framework_ok[@]}") from the framework. A framework crate joins that list in scripts/check-layout.sh only when every $half may use it."
          ;;
        feature)
          if [[ ${to[1]} != "${from[1]}" && ${to[2]} != api ]]; then
            echo "rule F4: a feature never uses another feature's ${to[2]}. Use its api, or a port in the framework."
          elif [[ ${to[2]} == api ]]; then
            return
          elif [[ $half == api ]]; then
            echo "rule F1: an api crate uses only framework crates on its list and api crates."
          elif [[ $half == core ]]; then
            echo "rule F2: a feature's core never uses a ui crate. Send an event instead."
          else
            echo "rule F3: a feature's ui never uses a core crate. Send a message instead."
          fi
          ;;
        *) echo "rule F4: a feature uses only the framework and api crates, never a front or the app." ;;
      esac
      ;;
  esac
}

# expect_rule <from folder> <to folder> <rule id, or nothing for allowed>
expect_rule() {
  local got
  got=$(rule_break "$1" "$2")
  got=${got%%:*}
  got=${got#rule }
  if [[ $got != "$3" ]]; then
    echo "$0: $1 using $2 should give '${3:-allowed}', got '${got:-allowed}'." >&2
    exit 1
  fi
}

expect_rule features/media/core features/notifications/core F4
expect_rule features/media/ui features/notifications/ui F4
expect_rule features/media/ui features/notifications/core F4
expect_rule features/media/core features/notifications/api ""
expect_rule features/media/ui features/media/api ""
expect_rule features/media/ui features/media/core F3
expect_rule features/media/core features/media/ui F2
expect_rule features/media/api features/media/core F1
expect_rule features/media/core framework/server F2
expect_rule features/media/api framework/server F1
expect_rule features/media/core fronts/cli F4
expect_rule features/media/core app F4
expect_rule features/media/core crates/novad M1
expect_rule framework/server features/media/api W1
expect_rule framework/server fronts/cli W1
expect_rule framework/server crates/nova-protocol M1
expect_rule framework/server framework/paths ""
expect_rule fronts/cli features/media/api ""
expect_rule fronts/cli features/media/core W2
expect_rule fronts/cli framework/server W2
expect_rule crates/novad features/media/core ""
expect_rule crates/novad framework/server ""
expect_rule app features/media/core ""
expect_rule tools/thing framework/server C1
expect_rule features/media framework/server C1
expect_rule fronts/cli/nested framework/server C1
expect_rule framework/server/extra framework/paths C1
expect_rule features/media/sub/core framework/server C1
expect_rule features/media/tests framework/server C1

# allowed_list <Cargo.toml> <outside|workspace>: the names listed under that
# header, MISSING without the header, or BAD <text> for a list on its line.
allowed_list() {
  awk -v header="# Allowed $2 crates:" '
    found && /^# - [^[:space:]]+$/ { print substr($0, 5); next }
    found && /^#[[:space:]]*-/ { print "BAD " $0; next }
    found { exit }
    index($0, header) == 1 {
      found = 1
      rest = substr($0, length(header) + 1)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", rest)
      if (rest != "" && rest != "none") { print "BAD " rest }
    }
    END { if (!found) print "MISSING" }
  ' "$1" | tr '\n' ' '
}

cd "$root"
metadata=$(cargo metadata --no-deps --offline --format-version 1)
workspace_names=$(jq -r '.packages[].name' <<<"$metadata")
declare -A folder_of=()
while IFS=$'\t' read -r name manifest; do
  folder=${manifest%/Cargo.toml}
  folder_of[$name]=${folder#"$root"/}
done < <(jq -r '.packages[] | [.name, .manifest_path] | @tsv' <<<"$metadata")

if ((${#folder_of[@]} == 0)); then
  echo "$0: cargo metadata listed no crates in $root." >&2
  exit 1
fi

status=0

# Every crate in a layout folder is a workspace member: cargo metadata never
# lists one that isn't, so none of the checks below would see it. A glob in
# `members` that matches no folder is an error to cargo, so the first crate
# of a kind adds its kind's glob. Test fixtures (under a tests/ folder) are not
# workspace crates.
declare -A is_member=()
for folder in "${folder_of[@]}"; do
  is_member[$folder]=1
done
while IFS= read -r manifest; do
  folder=${manifest#./}
  folder=${folder%/Cargo.toml}
  if [[ -z ${is_member[$folder]:-} ]]; then
    echo "$folder: not a workspace member (rule C2). Add its folder, or its kind's glob, to members in Cargo.toml."
    status=1
  fi
done < <(find app crates framework features fronts \( -name target -o -name tests \) -prune -o -name Cargo.toml -print 2>/dev/null | sort)

for name in "${!folder_of[@]}"; do
  folder=${folder_of[$name]}
  read -r -a layer <<<"$(layer_of "$folder")"
  if [[ ${layer[0]} == unknown ]]; then
    echo "$name ($folder): rule C1: every crate sits in app/, framework/<name>/, features/<name>/{api,core,ui}/, fronts/<name>/ or the old crates/. See $rules_doc."
    status=1
  elif [[ ${layer[0]} == feature && ! -f features/${layer[1]}/feature.toml ]]; then
    echo "$name ($folder): rule C3: a feature's crates sit beside its features/${layer[1]}/feature.toml. Name the folder after the feature's id."
    status=1
  fi
done

while IFS=$'\t' read -r name manifest deps; do
  outside=$(allowed_list "$manifest" outside)
  inside=$(allowed_list "$manifest" workspace)
  lists_ok=1
  if [[ $outside == *MISSING* || $inside == *MISSING* ]]; then
    echo "$manifest: rule D1: needs '# Allowed outside crates:' and '# Allowed workspace crates:' lines."
    lists_ok=0
  elif [[ $outside == *BAD* || $inside == *BAD* ]]; then
    echo "$manifest: rule D1: list allowed crates one per line as '# - name' under the header, or write 'none' on it."
    lists_ok=0
  fi
  ((lists_ok)) || status=1
  for dep in $deps; do
    if grep -xF -- "$dep" <<<"$workspace_names" >/dev/null; then
      allowed=$inside
      kind=workspace
      broken=$(rule_break "${folder_of[$name]}" "${folder_of[$dep]}")
      if [[ -n $broken ]]; then
        echo "$name (${folder_of[$name]}) depends on $dep (${folder_of[$dep]}): $broken See $rules_doc."
        status=1
      fi
    else
      allowed=$outside
      kind=outside
    fi
    # A broken list was reported above; its names can't be trusted.
    ((lists_ok)) || continue
    if ! tr -s ' ' '\n' <<<"$allowed" | grep -xF -- "$dep" >/dev/null; then
      echo "$name depends on $dep, which is not in its allowed $kind crates ($manifest, rule D2)."
      echo "  If it is really needed, add it to that list and say why in the PR."
      status=1
    fi
  done
done < <(jq -r '.packages[] | [.name, .manifest_path,
  ([.dependencies[] | select(.kind != "dev") | .name] | unique | join(" "))] | @tsv' <<<"$metadata")

exit "$status"
