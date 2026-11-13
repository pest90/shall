#!/bin/sh
# Serves the HTML report of a shall project on port 8000, for a demo; the
# `demo` service of compose.yaml runs it on the sample project
# fixtures/motor. Regenerates the report every SHALL_DEMO_INTERVAL seconds
# (default 5; 0: only at the start), so a change to the project shows after
# a reload. A broken state keeps the last good report.
#
#   shall-demo [<project directory, default /project>]
set -eu

project=${1:-/project}
interval=${SHALL_DEMO_INTERVAL:-5}
site=/srv/site
next=/srv/next
files=/srv/files
server=
last_error=

# As PID 1 the script gets no default handling of signals: stop at once.
stop() {
  [ -z "$server" ] || kill "$server" 2>/dev/null || true
  exit 0
}
trap stop INT TERM

report() {
  (cd "$project" && shall report "$@" --out "$next") 2>&1
}

# Prints an error once, not on every run while it lasts.
complain() {
  [ "$1" != "$last_error" ] || return 0
  last_error=$1
  echo "$1" >&2
  [ -z "$server" ] || echo "keeping the last report until this is fixed" >&2
}

# A report for every variant with an overview, or a single one for a
# project without variants.
generate() {
  rm -rf "$next"
  if ! log=$(report --all-variants); then
    case $log in
      *'has no [variant.*]'*)
        rm -rf "$next"
        log=$(report) || { complain "$log"; return 1; }
        ;;
      *) complain "$log"; return 1 ;;
    esac
  fi
  last_error=
  diff -r "$next" "$site" >/dev/null 2>&1 && return 0
  # Each file is renamed into place, which is atomic, so a request never
  # sees a missing or half-written file; then what is gone is removed.
  (cd "$next" && find . -type f) >"$files"
  (cd "$next" && find . -type d) | while read -r dir; do mkdir -p "$site/$dir"; done
  while read -r file; do mv "$next/$file" "$site/$file"; done <"$files"
  (cd "$site" && find . -type f) | grep -vxFf "$files" | while read -r file; do
    rm -f "$site/$file"
  done
  find "$site" -mindepth 1 -type d -empty -delete
}

mkdir -p "$site"
# The first report is also the check that the project can be read at all.
generate || exit 1
echo "serving the report of $project on port 8000"
busybox httpd -f -p 8000 -h "$site" &
server=$!

if [ "$interval" -gt 0 ]; then
  while kill -0 "$server" 2>/dev/null; do
    # In the background, so that a signal is handled without waiting.
    sleep "$interval" &
    wait $!
    generate || true
  done
fi
wait "$server"
