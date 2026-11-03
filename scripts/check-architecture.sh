#!/usr/bin/env bash
# The rules of docs/ARCHITECTURE.md section 4 that a grep can see. Each check counts violations in
# non-test code. A count above its budget in scripts/architecture-budget.txt fails, and a count
# below it fails too until the budget is lowered: budgets only go down, to 0.
#   --list        print the current counts in budget format
#   --new [BASE]  fail on any added line that breaks a rule, even when totals fall
set -euo pipefail
cd "$(dirname "$0")/.."
for input in scripts/architecture-budget.txt scripts/approved-crate-items.txt; do
  [ -f "$input" ] || { echo "check-architecture: $input is missing" >&2; exit 1; }
done

TEST_FILES='(^|/)tests?(/|\.rs$)|_tests?(/|\.rs$)|(^|/)test_[a-z_]+\.rs$|/benches/|/examples/|(^|/)build\.rs$'
src() { # $@ = path prefixes; prints non-test .rs files
  git ls-files --cached --others --exclude-standard -- "$@" | sort -u | grep -E '\.rs$' | grep -v -E "$TEST_FILES" || true
}
count() { # $1 = regex, files on stdin; drops inline test modules
  local re=$1 n=0 f c
  while read -r f; do
    [ -f "$f" ] || continue
    c=$(awk '/^[[:space:]]*#\[cfg\((all\()?test[,)]/ { t = 1; next }
             t && /mod [A-Za-z0-9_]+ *\{/ { exit }
             t { t = 0; next }
             { print }' "$f" | grep -c -E "$re" || true)
    n=$((n + c))
  done
  echo "$n"
}
deps() { # $1 = crate dir; prints its normal mimir dependencies
  [ -f "crates/$1/Cargo.toml" ] || return 0
  awk '/^\[dependencies\]/{on=1;next} /^\[/{on=0} on && /^mimir-[a-z-]+ *[.=]/{sub(/ *[.=].*/,"");print}' "crates/$1/Cargo.toml"
}
# Section 3: which crates each crate may depend on. A crate not listed here has no place.
allowed() {
  case "$1" in
    mimir-types) echo "" ;;
    mimir-parse-core) echo "mimir-types" ;;
    mimir-parse-rust) echo "mimir-types mimir-parse-core" ;;
    mimir-graph) echo "mimir-types" ;;
    mimir-resolve) echo "mimir-types mimir-graph mimir-parse-core" ;;
    mimir-resolve-ra) echo "mimir-types mimir-parse-core mimir-resolve" ;;
    mimir-edit) echo "mimir-types" ;;
    mimir-protocol) echo "mimir-types" ;;
    mimir-engine) echo "mimir-types mimir-graph mimir-parse-core mimir-parse-rust mimir-resolve mimir-edit mimir-protocol" ;;
    mimir-daemon) echo "mimir-protocol mimir-engine mimir-resolve" ;;
    mimir-client) echo "mimir-types mimir-protocol" ;;
    mimir-mcp) echo "mimir-protocol mimir-client" ;;
    mimir-view-wire) echo "" ;;
    mimir-view) echo "mimir-types mimir-protocol mimir-client mimir-view-wire" ;;
    mimir-view-page) echo "mimir-view-wire" ;;
    mimir-cli) echo "mimir-types mimir-protocol mimir-client mimir-daemon mimir-mcp mimir-view mimir-resolve mimir-resolve-ra" ;;
    *) echo "NOT-IN-SPEC" ;;
  esac
}
wrong_deps() { # counts dependencies, and crates, the spec does not allow
  local n=0 c a d
  for c in crates/*/; do
    c=$(basename "$c"); a=" $(allowed "$c") "
    case "$a" in *NOT-IN-SPEC*) n=$((n + 1)); continue ;; esac
    for d in $(deps "$c"); do case "$a" in *" $d "*) ;; *) n=$((n + 1)) ;; esac; done
  done
  echo "$n"
}
unused_deps() { # dependencies declared but not named in the crate's source
  local n=0 c d
  for c in crates/*/; do
    c=$(basename "$c")
    for d in $(deps "$c"); do
      grep -rqE "\b$(echo "$d" | tr - _)\b" "crates/$c/src" || n=$((n + 1))
    done
  done
  echo "$n"
}
DOMAIN='crates/mimir-types crates/mimir-graph crates/mimir-parse-core crates/mimir-parse-rust'
TRANSPORT='crates/mimir-cli crates/mimir-mcp crates/mimir-daemon crates/mimir-client crates/mimir-view'

now() {
  case "$1" in
    # Rule 1: the domain is pure
    domain_io) src $DOMAIN | count 'std::fs|tokio::|std::process|Command::new|env::var|Utc::now\(|SystemTime::now\(|Instant::now\(' ;;
    # Section 3: only the allowed dependencies, none unused
    wrong_deps) wrong_deps ;;
    unused_deps) unused_deps ;;
    # Rule 3: transports reach the world only through the client or the engine
    transport_reach) src $TRANSPORT | grep -v -E '^crates/mimir-(client|daemon)/|^crates/mimir-cli/src/commands/daemon\.rs$' | count 'std::fs|tokio::fs|Command::new|mimir_(graph|resolve|engine|parse_core|parse_rust|edit)\b' ;;
    # Rule 5
    allow_attrs) src crates | count '#!?\[allow\(' ;;
    unwrap_expect) src crates | count '\.unwrap\(\)|\.expect\(' ;;
    swallowed) src crates | count 'let _ = |\.ok\(\);' ;;
    debug_names) src crates | count 'format!\("\{:\?\}"' ;;
    string_errors) src crates | count 'to_string\(\)\.contains\(|err_str' ;;
    anyhow_libs) src crates | grep -v '^crates/mimir-cli/' | count 'anyhow' ;;
    env_below_root) src crates | grep -v '^crates/mimir-cli/' | count 'env::var(_os)?\(' ;;
    # Rule 7
    pub_super) src crates | count 'pub\(super\)' ;;
    super_paths) src crates | count '^[^/"]*(^|[^a-z_])super::' ;;
    pub_in_path) src crates | count 'pub\(in ' ;;
    path_attrs) src crates | count '#\[path' ;;
    pub_crate) src crates | count '^[[:space:]]*pub\(crate\)' ;;
    *) echo "" ;;
  esac
}
CHECKS='allow_attrs anyhow_libs debug_names domain_io env_below_root path_attrs pub_crate pub_in_path pub_super string_errors super_paths swallowed transport_reach unused_deps unwrap_expect wrong_deps'

if [ "${1:-}" = "--new" ]; then
  base=${2:-HEAD}
  re='pub\(super\)|pub\(crate\)|pub\(in |#\[path|(^|[^a-z_*])super::|#!?\[allow\(|\.unwrap\(\)|\.expect\(|let _ = |\.ok\(\);|format!\("\{:\?\}"|to_string\(\)\.contains\(|err_str'
  approved=$(grep -v -E '^[[:space:]]*(#|$)' scripts/approved-crate-items.txt | awk '{print $1 " " $2}' || true)
  added() { # $1 = file; prints added lines before its inline test module
    local f=$1 cut
    cut=$(awk '/^[[:space:]]*#\[cfg\((all\()?test/ { t = NR; next }
               t && NR == t + 1 && /mod [A-Za-z0-9_]+ *\{/ { print t; exit }
               { t = 0 }' "$f")
    cut=${cut:-999999}
    if git ls-files --error-unmatch "$f" >/dev/null 2>&1; then
      git diff -U0 "$base" -- "$f"
    else
      git diff --no-index -U0 /dev/null "$f" || true
    fi | awk -v cut="$cut" -v f="$f" '
      /^@@/ { split($3, a, ","); n = substr(a[1], 2) + 0; next }
      /^\+\+\+/ { next }
      /^\+/ { if (n < cut) print f ": " substr($0, 2); n++ }'
  }
  hits=$( { for f in $( { git diff --name-only "$base" -- 'crates/*.rs'; git ls-files --others --exclude-standard -- 'crates/*.rs'; } | sort -u | grep -v -E "$TEST_FILES" || true); do
              if [ -f "$f" ]; then added "$f"; fi
            done
          } | { grep -E "$re|anyhow" || true; } | { grep -v -E '^crates/mimir-cli/.*anyhow' || true; } | { grep -v -E ': *use super::\*;' || true; } | { grep -v -E ': *//' || true; })
  if [ -n "$approved" ] && [ -n "$hits" ]; then
    while read -r file item; do
      hits=$(grep -v -E "^$file: .*pub\(crate\).*\b$item\b" <<<"$hits" || true)
    done <<<"$approved"
  fi
  if [ -n "$hits" ]; then
    echo "error: new violations against $base (docs/ARCHITECTURE.md section 4):"
    echo "$hits" | head -60
    exit 1
  fi
  echo "no new violations against $base"
  exit 0
fi

if [ "${1:-}" = "--list" ]; then
  for k in $CHECKS; do echo "$k $(now "$k")"; done
  exit 0
fi

fail=0
for k in $CHECKS; do
  budget=$(awk -v k="$k" '$1 == k { print $2 }' scripts/architecture-budget.txt)
  [ -n "$budget" ] || { echo "error: $k has no budget"; fail=1; continue; }
  n=$(now "$k")
  if [ "$n" -gt "$budget" ]; then
    echo "error: $k has $n violations, budget $budget (docs/ARCHITECTURE.md section 4)"; fail=1
  elif [ "$n" -lt "$budget" ]; then
    echo "error: $k is down to $n; lower its budget in scripts/architecture-budget.txt"; fail=1
  fi
done
if [ "$fail" = 0 ]; then echo "architecture check passed"; fi
exit "$fail"
