package ghrg.repos

default allow := false

allow if {
    input.name != ""
}

output := {
    "Name": input.name,
    "Team": input.contexts.repo_properties.Team,
    "CostCenter": input.contexts.repo_properties.CostCenter,
    "Archived": input.archived,
}

meta := {
    "selected_fields": ["Name", "Team", "CostCenter", "Archived"],
    "policy": "project-summary",
}
