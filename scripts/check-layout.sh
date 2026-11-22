#!/usr/bin/env bash
# Fails when a crate sits outside Nova's layout or depends on a crate it may
# not use, naming the broken rule and what to do instead. The rules and the
# folder kinds are in AGENTS.md ("Layout"); the rule ids printed here are the
# ones in its table.
#
# The repo holds two cargo workspaces, one per side: the daemon side's at
# the root (app, framework, fronts, each feature's api and core) and the UI
# side's at app-ui/ (app-ui, framework-ui, each feature's ui), the only one
# whose crates may use GPUI. The script reads both.
#
# Three checks:
#   - The layout: a crate's folder decides what it may use (layer_of below)
#     and which workspace it belongs to, so a new crate can't opt out.
#   - The two workspaces' shared tables ([workspace.package] and
#     [workspace.lints.*]) are the same, so one lint set holds everywhere.
#   - Allowed lists: each Cargo.toml lists the outside and workspace crates it
#     may use under its "# Allowed outside crates:" and "# Allowed workspace
#     crates:" lines, one "# - name" line each, or "none" on the header line,
#     so adding a dependency is a visible line in a PR.
# Dev-dependencies are checked for one rule only (T1): a feature's tests never
# use another feature's core or ui unless DEV_EXCEPTIONS lists the pair with
# its reason. Before reading the workspace, the script
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
# The light -info crates hold wire types only (rule I1), so every kind may
# use them. The ui list is what the whole UI side may use from framework/:
# a feature's ui, framework-ui/ and app-ui/. Its own shared parts are
# framework-ui/, which only the UI side may use.
FRAMEWORK_FOR_API=(app-info carrier daemon-info desktop-info host-info theme-info)
FRAMEWORK_FOR_CORE=(app-info apps base carrier compositor daemon-info desktop-info host-info hub icons inotify log mime modules paths process session-bus settings socket theme-info watch)
FRAMEWORK_FOR_UI=(app-info carrier daemon-info desktop-info host-info theme-info)
FRAMEWORK_FOR_FRONT=(app-info base carrier daemon-info desktop-info host-info paths quickshell theme-info)
# The UI side's workspace root, relative to the repo's.
UI_WORKSPACE=app-ui

# The dev-dependencies a feature's crate may have on another feature's core
# or ui (rule T1), as "<crate> <dependency>", each with its reason. Every
# entry is a test that a fake from the framework could replace.
DEV_EXCEPTIONS=(
  # Peripherals' low battery alert tests post to a real notification server.
  "nova-peripherals nova-notifications"
  # Recording's tests fill its SavedFiles port the way the app does.
  "nova-recording nova-screenshot"
)

# layer_of <crate folder relative to the root>: prints the crate's kind and
# its parts ("feature <name> <half>", "framework <name>", "app-ui", ...).
layer_of() {
  # A case pattern's * also matches /, so each kind is matched with its
  # exact depth: framework/a/b is outside the layout, not "framework a/b".
  local -a parts
  IFS=/ read -r -a parts <<<"$1"
  case ${#parts[@]}:${parts[0]} in
    1:app) echo app ;;
    1:app-ui) echo app-ui ;;
    2:fronts) echo front ;;
    2:framework) echo "framework ${parts[1]}" ;;
    2:framework-ui) echo "framework-ui ${parts[1]}" ;;
    3:features)
      case ${parts[2]} in
        api | core | ui) echo "feature ${parts[1]} ${parts[2]}" ;;
        *) echo "unknown $1" ;;
      esac
      ;;
    *) echo "unknown $1" ;;
  esac
}

# side_of <crate folder>: "ui" for a crate of the UI side's workspace (the
# ones that may use GPUI), "daemon" for one of the root's, nothing for a
# folder outside the layout.
side_of() {
  local -a layer
  read -r -a layer <<<"$(layer_of "$1")"
  case ${layer[0]}:${layer[2]:-} in
    app-ui:* | framework-ui:* | feature:ui) echo ui ;;
    unknown:*) ;;
    *) echo daemon ;;
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
    echo "rule C1: every crate sits in app/, app-ui/, framework/<name>/, framework-ui/<name>/, features/<name>/{api,core,ui}/, or fronts/<name>/."
    return
  fi
  local ui_side="rule U1: the daemon side (app, framework, fronts, a feature's api and core) never uses a UI crate (app-ui, framework-ui, a feature's ui). Send an event instead."
  # Between two features' crates the F rules below say it more exactly.
  if [[ $(side_of "$1") == daemon && $(side_of "$2") == ui && ${from[0]}:${to[0]} != feature:feature ]]; then
    echo "$ui_side"
    return
  fi
  case ${from[0]} in
    app) return ;;
    app-ui)
      case ${to[0]}:${to[2]:-} in
        framework-ui:* | feature:api | feature:ui) return ;;
        framework:*) in_list "${to[1]}" "${FRAMEWORK_FOR_UI[@]}" && return ;;
      esac
      echo "rule U3: the UI program uses only ui and api crates, framework-ui and $(framework_names "${FRAMEWORK_FOR_UI[@]}") (the list in scripts/check-layout.sh). Send novad a message instead."
      ;;
    framework-ui)
      case ${to[0]} in
        framework-ui) return ;;
        framework) in_list "${to[1]}" "${FRAMEWORK_FOR_UI[@]}" && return ;;
      esac
      echo "rule U2: a framework-ui crate uses only other framework-ui crates and $(framework_names "${FRAMEWORK_FOR_UI[@]}") (the list in scripts/check-layout.sh), and never names a feature. Let the feature's ui plug in instead."
      ;;
    framework)
      if [[ ${to[0]} == framework ]]; then
        # An -info crate's event and topic need nova-carrier; its types may
        # carry another -info crate's.
        if [[ ${from[1]} == *-info && ${to[1]} != carrier && ${to[1]} != *-info ]]; then
          echo "rule I1: an -info crate holds wire types only, so it uses only nova-carrier and other -info crates. Move the code to the crate that builds the types."
        fi
        return
      fi
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
        framework-ui)
          # A ui's; an api or core using one was rule U1 above.
          return
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
        *) echo "rule F4: a feature uses only the framework and api crates, never a front, the app or the UI program." ;;
      esac
      ;;
  esac
}

# dev_rule_break <from folder> <to folder>: prints the rule a dev-dependency
# from the first crate on the second breaks, if any (before DEV_EXCEPTIONS).
dev_rule_break() {
  local -a from to
  read -r -a from <<<"$(layer_of "$1")"
  read -r -a to <<<"$(layer_of "$2")"
  if [[ ${from[0]} == feature && ${to[0]} == feature && ${to[1]} != "${from[1]}" && ${to[2]} != api ]]; then
    echo "rule T1: a feature's tests never use another feature's ${to[2]}. Use a fake from the framework, or list the pair in DEV_EXCEPTIONS in scripts/check-layout.sh with its reason."
  fi
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
expect_rule framework/server features/media/api W1
expect_rule framework/server fronts/cli W1
expect_rule framework/server framework/paths ""
expect_rule framework/theme-info framework/carrier ""
expect_rule framework/daemon-info framework/desktop-info ""
expect_rule framework/theme-info framework/paths I1
expect_rule framework/theme-info features/media/api W1
expect_rule fronts/cli features/media/api ""
expect_rule fronts/cli features/media/core W2
expect_rule fronts/cli framework/server W2
expect_rule fronts/cli app W2
expect_rule crates/nova-cli framework/server C1
expect_rule app features/media/core ""
expect_rule tools/thing framework/server C1
expect_rule features/media framework/server C1
expect_rule fronts/cli/nested framework/server C1
expect_rule framework/server/extra framework/paths C1
expect_rule features/media/sub/core framework/server C1
expect_rule features/media/tests framework/server C1
expect_rule app-ui features/media/ui ""
expect_rule app-ui features/media/api ""
expect_rule app-ui framework-ui/stage ""
expect_rule app-ui framework/carrier ""
expect_rule app-ui framework/server U3
expect_rule app-ui features/media/core U3
expect_rule app-ui fronts/cli U3
expect_rule app-ui app U3
expect_rule app-ui/nested framework/carrier C1
expect_rule framework-ui/stage framework-ui/look ""
expect_rule framework-ui/stage framework/theme-info ""
expect_rule framework-ui/stage framework/server U2
expect_rule framework-ui/stage features/media/api U2
expect_rule framework-ui/stage features/media/ui U2
expect_rule framework-ui/stage app-ui U2
expect_rule framework-ui/stage/extra framework/carrier C1
expect_rule features/media/ui framework-ui/stage ""
expect_rule features/media/ui framework/carrier ""
expect_rule features/media/ui framework/server F3
expect_rule features/media/ui app-ui F4
expect_rule features/media/core framework-ui/stage U1
expect_rule features/media/api framework-ui/stage U1
expect_rule features/media/api features/media/ui F1
expect_rule framework/server framework-ui/stage U1
expect_rule framework/server app-ui U1
expect_rule fronts/cli framework-ui/stage U1
expect_rule fronts/cli features/media/ui U1
expect_rule app features/media/ui U1
expect_rule app framework-ui/stage U1
expect_rule app app-ui U1

# expect_dev_rule <from folder> <to folder> <rule id, or nothing for allowed>
expect_dev_rule() {
  local got
  got=$(dev_rule_break "$1" "$2")
  got=${got%%:*}
  got=${got#rule }
  if [[ $got != "$3" ]]; then
    echo "$0: $1 dev-using $2 should give '${3:-allowed}', got '${got:-allowed}'." >&2
    exit 1
  fi
}

expect_dev_rule features/media/core features/notifications/core T1
expect_dev_rule features/media/core features/notifications/ui T1
expect_dev_rule features/media/core features/notifications/api ""
expect_dev_rule features/media/core features/media/api ""
expect_dev_rule features/media/core framework/hub ""
expect_dev_rule app features/media/core ""

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

# shared_table <Cargo.toml> <table>: the table's lines and those of its
# sub-tables, without comments and blank lines.
shared_table() {
  awk -v table="$2" '
    /^\[/ { inside = ($0 == "[" table "]" || index($0, "[" table ".") == 1) }
    inside && !/^[[:space:]]*(#|$)/
  ' "$1"
}

cd "$root"
# Both workspaces' crates as one list, each with the side whose workspace
# lists it. A workspace without a UI side (a throwaway one in the tests)
# has only the root's.
metadata=$(cargo metadata --no-deps --offline --format-version 1 | jq '[.packages[] | .side = "daemon"]')
if [[ -f $UI_WORKSPACE/Cargo.toml ]]; then
  ui_metadata=$(cargo metadata --no-deps --offline --format-version 1 --manifest-path "$UI_WORKSPACE/Cargo.toml")
  metadata=$(jq --argjson ui "$ui_metadata" '. + [$ui.packages[] | .side = "ui"]' <<<"$metadata")
fi
metadata=$(jq '{packages: .}' <<<"$metadata")
workspace_names=$(jq -r '.packages[].name' <<<"$metadata")
declare -A folder_of=()
declare -A member_side=()
while IFS=$'\t' read -r name manifest side; do
  folder=${manifest%/Cargo.toml}
  folder_of[$name]=${folder#"$root"/}
  member_side[${folder#"$root"/}]=$side
done < <(jq -r '.packages[] | [.name, .manifest_path, .side] | @tsv' <<<"$metadata")

if ((${#folder_of[@]} == 0)); then
  echo "$0: cargo metadata listed no crates in $root." >&2
  exit 1
fi

status=0

# The lint set and the package fields are one for both workspaces: cargo
# can't share a table between two, so the UI side's root keeps a copy.
if [[ -f $UI_WORKSPACE/Cargo.toml ]]; then
  for table in workspace.package workspace.lints; do
    if ! diff <(shared_table Cargo.toml "$table") <(shared_table "$UI_WORKSPACE/Cargo.toml" "$table") >/dev/null; then
      echo "$UI_WORKSPACE/Cargo.toml: rule S1: its [$table] differs from Cargo.toml's. Make the same change in both."
      status=1
    fi
  done
fi

# No crate is published: cargo-deny tells a path dependency from a wildcard
# version by it (app-ui/deny.toml), so one crate without it fails the UI
# side's check as soon as a UI crate uses it.
while IFS= read -r manifest; do
  echo "${manifest#"$root"/}: rule S2: the crate can be published. Add \`publish.workspace = true\` to its [package]."
  status=1
done < <(jq -r '.packages[] | select(.publish != []) | .manifest_path' <<<"$metadata")

# Every crate in a layout folder is a member of its side's workspace: cargo
# metadata never lists one that isn't, so none of the checks below would see
# it. A glob in `members` that matches no folder is an error to cargo, so
# the first crate of a kind adds its kind's glob. Test fixtures (under a
# tests/ folder) are not workspace crates.
while IFS= read -r manifest; do
  folder=${manifest#./}
  folder=${folder%/Cargo.toml}
  side=$(side_of "$folder")
  [[ ${member_side[$folder]:-} == "${side:-daemon}" ]] && continue
  if [[ $side == ui ]]; then
    echo "$folder: not a member of the UI side's workspace (rule C2). Add its folder, or its kind's glob, to members in $UI_WORKSPACE/Cargo.toml, and name that workspace in its [package] (workspace = \"<path to $UI_WORKSPACE>\")."
  else
    echo "$folder: not a workspace member (rule C2). Add its folder, or its kind's glob, to members in Cargo.toml."
  fi
  status=1
done < <(find app "$UI_WORKSPACE" framework framework-ui features fronts \( -name target -o -name tests \) -prune -o -name Cargo.toml -print 2>/dev/null | sort)

for name in "${!folder_of[@]}"; do
  folder=${folder_of[$name]}
  read -r -a layer <<<"$(layer_of "$folder")"
  if [[ ${layer[0]} == unknown ]]; then
    echo "$name ($folder): rule C1: every crate sits in app/, app-ui/, framework/<name>/, framework-ui/<name>/, features/<name>/{api,core,ui}/, or fronts/<name>/. See $rules_doc."
    status=1
  elif [[ ${member_side[$folder]} != "$(side_of "$folder")" ]]; then
    # Reached through a members line that names the folder itself.
    echo "$name ($folder): rule C2: it belongs to the $(side_of "$folder") side's workspace, not the ${member_side[$folder]} side's. See $rules_doc."
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
      if [[ $dep == gpui || $dep == gpui_* ]] && [[ $(side_of "${folder_of[$name]}") != ui ]]; then
        echo "$name (${folder_of[$name]}) depends on $dep: rule U1: only the UI side (app-ui, framework-ui, a feature's ui) uses GPUI. See $rules_doc."
        status=1
      fi
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

declare -A dev_exception=()
for pair in "${DEV_EXCEPTIONS[@]}"; do
  dev_exception[$pair]=1
done
declare -A dev_exception_used=()
while IFS=$'\t' read -r name deps; do
  for dep in $deps; do
    [[ -n ${folder_of[$dep]:-} ]] || continue
    broken=$(dev_rule_break "${folder_of[$name]}" "${folder_of[$dep]}")
    [[ -n $broken ]] || continue
    if [[ -n ${dev_exception["$name $dep"]:-} ]]; then
      dev_exception_used["$name $dep"]=1
      continue
    fi
    echo "$name (${folder_of[$name]}) dev-depends on $dep (${folder_of[$dep]}): $broken See $rules_doc."
    status=1
  done
done < <(jq -r '.packages[] | [.name,
  ([.dependencies[] | select(.kind == "dev") | .name] | unique | join(" "))] | @tsv' <<<"$metadata")
# An entry whose crate is in this workspace but no longer needs it is stale.
for pair in "${DEV_EXCEPTIONS[@]}"; do
  if [[ -n ${folder_of[${pair%% *}]:-} && -z ${dev_exception_used[$pair]:-} ]]; then
    echo "DEV_EXCEPTIONS lists '$pair', which no longer breaks rule T1 (scripts/check-layout.sh). Remove the entry."
    status=1
  fi
done

exit "$status"
