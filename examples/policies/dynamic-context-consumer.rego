package ghrg.repos

default allow := false

allow if {
    input.name != ""
}

output := {
    "Name": input.name,
    "Archived": input.archived,
    "RecentCommits": count(input.contexts.recent_commits),
    "WorkflowFiles": count(input.contexts.workflow_files),
}

meta := {
    "policy": "dynamic-context-consumer",
    "selected_fields": [
        "Name",
        "Archived",
        "RecentCommits",
        "WorkflowFiles",
    ],
}
