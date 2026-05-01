# ```ghrg
# name: dynamic-context-meta-seed
# description: Seed dynamic context parameters for later policies
# contexts: []
# ```

package ghrg.repos

default allow := true

output := input

meta := {
  "recent_commit_limit": 2,
  "workflow_glob": ".github/workflows/*.yml",
}
