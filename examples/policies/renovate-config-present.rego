package ghrg.repos

default allow := false
default renovate_configured := false
default selected_config_path := null

allow if {
    input.name != ""
}

matching_file(path) := file if {
    some file in input.contexts.renovate_config_files
    file.path == path
}

selected_config := file if {
    file := matching_file("renovate.json")
} else := file if {
    file := matching_file("renovate.json5")
} else := file if {
    file := matching_file(".github/renovate.json")
} else := file if {
    file := matching_file(".github/renovate.json5")
} else := file if {
    file := matching_file(".gitlab/renovate.json")
} else := file if {
    file := matching_file(".gitlab/renovate.json5")
} else := file if {
    file := matching_file(".renovaterc")
} else := file if {
    file := matching_file(".renovaterc.json")
} else := file if {
    file := matching_file(".renovaterc.json5")
}

renovate_configured if {
    selected_config.path != ""
}

selected_config_path := selected_config.path if {
    renovate_configured
}

output := {
    "Name": input.name,
    "Archived": input.archived,
    "RenovateConfigured": renovate_configured,
    "RenovateConfigPath": selected_config_path,
}

meta := {
    "policy": "renovate-config-present",
    "selected_fields": [
        "Name",
        "Archived",
        "RenovateConfigured",
        "RenovateConfigPath",
    ],
}
