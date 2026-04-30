# ```ghrg
# name: filter-active
# description: Keep only non-archived repositories
# contexts: []
# ```

package ghrg.repos

default allow := false

allow if {
    input.archived == false
}

output := input

meta := {
    "reason": "repository is active",
    "policy": "filter-active",
}
