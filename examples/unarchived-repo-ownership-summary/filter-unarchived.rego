# ```ghrg
# name: filter-unarchived
# description: Keep only unarchived repositories before enrichment
# contexts: []
# ```

package ghrg.repos

default allow := false

allow if {
    input.archived == false
}

output := input

meta := {
    "reason": "repository is not archived",
    "policy": "filter-unarchived",
}
