package ghrg.repos

default allow := false
default renovate_configured := false
default selected_config_path := null
default repo_archived := false
default minimum_release_age_enabled := false
default minimum_release_age_over_7_days := false
default minimum_release_age_policy_satisfied := false

allow if {
    repo_name != ""
}

repo_name := name if {
    name := input.Name
    name != ""
} else := name if {
    name := input.name
    name != ""
}

repo_archived := archived if {
    archived := input.Archived
} else := archived if {
    archived := input.archived
}

selected_config_file := file if {
    some file in input.contexts.selected_renovate_config
    file.path == input.RenovateConfigPath
}

selected_config_path := path if {
    path := input.RenovateConfigPath
    path != null
}

renovate_configured if {
    input.RenovateConfigured
}

renovate_configured if {
    selected_config_path != null
}

minimum_release_age_enabled if {
    content := selected_config_file.content
    content != null
    regex.match(`(?is)["']?minimumReleaseAge["']?\s*:`, content)
}

minimum_release_age_over_7_days if {
    content := selected_config_file.content
    content != null
    regex.match(`(?is)["']?minimumReleaseAge["']?\s*:\s*["'](?:[8-9]|[1-9][0-9]+)\s+days?["']`, content)
}

minimum_release_age_over_7_days if {
    content := selected_config_file.content
    content != null
    regex.match(`(?is)["']?minimumReleaseAge["']?\s*:\s*["'][1-9][0-9]*\s+(weeks?|months?|years?)["']`, content)
}

minimum_release_age_policy_satisfied if {
    renovate_configured
    minimum_release_age_over_7_days
}

output := {
    "Name": repo_name,
    "Archived": repo_archived,
    "RenovateConfigured": renovate_configured,
    "RenovateConfigPath": selected_config_path,
    "MinimumReleaseAgeEnabled": minimum_release_age_enabled,
    "MinimumReleaseAgeOver7Days": minimum_release_age_over_7_days,
    "MinimumReleaseAgePolicySatisfied": minimum_release_age_policy_satisfied,
}

meta := {
    "policy": "renovate-minimum-release-age",
    "selected_fields": [
        "Name",
        "Archived",
        "RenovateConfigured",
        "RenovateConfigPath",
        "MinimumReleaseAgeEnabled",
        "MinimumReleaseAgeOver7Days",
        "MinimumReleaseAgePolicySatisfied",
    ],
}
