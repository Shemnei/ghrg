package ghrg.repos

default allow := false
default minimum_release_age_enabled := false
default minimum_release_age_over_7_days := false
default minimum_release_age_policy_satisfied := false

allow if {
    input.Name != ""
}

matching_file(path) := file if {
    some file in input.contexts.renovate_config_files
    file.path == path
}

selected_config_file := file if {
    path := input.RenovateConfigPath
    path != null
    file := matching_file(path)
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
    input.RenovateConfigured
    minimum_release_age_over_7_days
}

output := {
    "Name": input.Name,
    "Archived": input.Archived,
    "RenovateConfigured": input.RenovateConfigured,
    "RenovateConfigPath": input.RenovateConfigPath,
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
