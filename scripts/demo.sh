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

# A report for every variant if the project has some, with an overview.
generate() {
  rm -rf "$next"
  set --
  if grep -q '^\[variant\.' "$project/shall.toml" 2>/dev/null; then
    set -- --all-variants
  fi
  if ! log=$(cd "$project" && shall report "$@" --out "$next" 2>&1); then
    echo "$log" >&2
    return 1
  fi
  rm -rf "${site:?}"/*
  cp -R "$next"/. "$site"/
}

mkdir -p "$site"
# The first report is also the check that the project can be read at all.
generate
echo "serving the report of $project on port 8000"
busybox httpd -f -p 8000 -h "$site" &
server=$!
trap 'kill "$server"' INT TERM

if [ "$interval" -gt 0 ]; then
  while kill -0 "$server" 2>/dev/null; do
    sleep "$interval"
    generate || echo "keeping the last report" >&2
  done
fi
wait "$server"
