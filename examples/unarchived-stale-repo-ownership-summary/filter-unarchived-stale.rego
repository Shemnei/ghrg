# ```ghrg
# name: filter-unarchived-stale
# description: Keep only unarchived repositories with no pushes in roughly six months
# contexts: []
# ```

package ghrg.repos

default allow := false

stale_after_ns := time.parse_duration_ns("4320h")

allow if {
    input.archived == false
    time.parse_rfc3339_ns(input.github.pushed_at) <= time.now_ns() - stale_after_ns
}

output := input

meta := {
    "reason": "repository is unarchived and has no pushes in roughly six months",
    "policy": "filter-unarchived-stale",
}
