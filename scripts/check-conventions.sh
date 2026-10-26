#!/usr/bin/env bash
# Checks a branch name and a pull request title against CONTRIBUTING.md.
# Usage: scripts/check-conventions.sh <branch> <title>
set -euo pipefail

branch=$1
title=$2
status=0

fail() {
    echo "error: $1" >&2
    status=1
}

branch_re='^(feature|fix|docs|refactor|chore)/[a-z0-9][a-z0-9.-]*$'
[[ $branch =~ $branch_re ]] ||
    fail "branch '$branch' does not match $branch_re"

types='feat|fix|docs|refactor|perf|test|build|ci|chore|revert'
scopes='core|md|typst|scan|junit|trace|report|pdf|cli|plugin|build-check|deps|ci|release'
title_re="^($types)(\(($scopes)\))?!?: (.+)$"
if [[ $title =~ $title_re ]]; then
    summary=${BASH_REMATCH[4]}
    [[ $summary =~ ^[A-Z] ]] && fail "summary must start in lower case: '$summary'"
    [[ $summary == *. ]] && fail "summary must not end with a period: '$summary'"
    (( ${#summary} <= 72 )) || fail "summary is longer than 72 characters (${#summary})"
else
    fail "title '$title' is not '<type>(<scope>): <summary>' with type in {$types} and scope in {$scopes}"
fi

exit $status
