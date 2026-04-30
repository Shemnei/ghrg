package ghrg.repos

default allow := true

output := {
    "Name": input.name,
    "Team": input.contexts.repo_properties.Team,
    "CodeOwner": input.contexts.repo_properties.CodeOwner,
    "Public": input.visibility == "public",
}

meta := {
    "selected_fields": ["Name", "Team", "CodeOwner", "Public"],
    "policy": "repo-ownership-summary",
}
