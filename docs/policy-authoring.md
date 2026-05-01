# Policy Authoring

This guide explains the `ghrg` policy model, how multiple `--policy` files are evaluated, how contexts are requested, and how to iterate locally before scanning GitHub.

## The policy model

A `ghrg` policy is a Rego file in the `ghrg.repos` package that can define up to three public rules:

- `allow`: whether the current object stays in the pipeline
- `output`: the visible object passed to the next policy and eventually rendered
- `meta`: optional debug or trace metadata

At minimum, a useful policy normally sets `allow` and `output`.

Example filter:

```rego
package ghrg.repos

default allow := false

allow if {
    input.archived == false
}

output := input
```

Example projection:

```rego
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
```

## How `allow` works

- if `allow` evaluates to `true`, the object stays in the chain
- if `allow` evaluates to `false` or is undefined, the object is dropped
- when a policy drops the object, later policies do not run

That makes `allow` the main place for filter logic.

Common pattern:

```rego
default allow := false

allow if {
    some_condition
}
```

## How `output` works

`output` is the visible object produced by the policy.

- if `output` is present, its JSON object becomes the next input in the chain
- if `output` is not present, `ghrg` falls back to the current input object
- the final visible output is what `json`, `csv`, and `pretty` render

This means policies can either:

- filter only: `output := input`
- transform only: build a smaller or reshaped object
- do both: filter first, then project a final report shape

`output` must effectively serialize to a JSON object for the CLI output model.

## How `meta` works

`meta` is optional.

- it does not affect filtering
- it does not change the visible output
- `ghrg` evaluates it and keeps it alongside the policy result for tooling/debug use

Use it for things like:

- selected field lists
- policy names
- debug reasoning
- traceability notes

In the current CLI, the `meta` rule is mostly a debug-oriented hook. The regular `pretty`, `json`, and `csv` outputs only render the visible `output` object, and the built-in commands do not yet expose policy `meta` as prominently as policy-sidecar metadata.

## Metadata and context requests

Policies can declare metadata either:

- embedded at the top of the `.rego` file
- in a sidecar `.ghrg.yaml` file next to the policy

Do not use both for the same policy file.

### Embedded metadata

```rego
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
```

### Sidecar metadata

`project-summary.rego`

```rego
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
```

`project-summary.ghrg.yaml`

```yaml
name: project-summary
description: Project a compact repo summary for local testing
contexts:
  - name: repo_properties
    type: properties
    names:
      - Team
      - CostCenter
```

## Named contexts

The public docs and starter examples prefer named contexts.

Example:

```yaml
contexts:
  - name: repo_properties
    type: properties
    names:
      - Team
      - CodeOwner
```

That context is available in Rego as:

```rego
input.contexts.repo_properties
```

This is the preferred convention because it makes policy input shape explicit and stable.

## Context request model

Contexts declare extra data that `ghrg` should make available to the policy.

Current kinds include:

- `properties`
- `languages`
- `branches`
- `commits`
- `files`
- `contributors`
- `workflow_runs`

Example:

```yaml
contexts:
  - name: repo_properties
    type: properties
    names:
      - Team
      - CodeOwner
```

During `ghrg repos`, requested contexts are fetched from GitHub and attached under `input.contexts`.

During `ghrg policy test` and `ghrg policy trace`, requested contexts are populated with local sample values derived from the input object.

### Dynamic context fields

Context fields can be static literals or dynamic references.

Static:

```yaml
contexts:
  - type: commits
    limit: 5
```

Dynamic:

```yaml
contexts:
  - name: recent_commits
    type: commits
    limit:
      from: input.max_commit_limit
      default: 5
    ref:
      from: env.GHRG_REF
      default: main
```

Reference shape:

- `from` (required): source path
- `default` (optional): fallback when source is missing

Supported `from` sources:

- `input` or `input.<path>`
- `env.<VAR_NAME>`
- `meta.last` or `meta.last.<path>`
- `meta.policies` or `meta.policies.<policy_key>[.<path>]`

Policy keys under `meta.policies` are available by:

- metadata name (when `name` is set in policy metadata)
- policy path

Resolution behavior:

- values are resolved per policy step, before context fetch
- missing source without `default` fails fast
- type mismatches fail with a context field resolution error

## Multiple `--policy` files

When you pass multiple policy files, `ghrg` evaluates them in the order you provide them.

Flow:

1. start with the original repository input
2. attach any contexts requested by the current policy
3. evaluate `allow`, `output`, and `meta`
4. if kept, use the policy's `output` as the next input
5. if dropped, stop immediately

Practical effect:

- early policies are usually cheap filters
- later policies are usually enrich/projection steps
- a later policy only sees fields preserved by earlier `output` rules

Recommended pattern:

1. filter first
2. enrich or summarize second
3. keep the final output narrow and report-friendly

## Drop vs transform behavior

Think of each policy as answering two questions:

- should this object continue? (`allow`)
- what shape should continue? (`output`)

Examples:

- filter-only policy: `allow` decides, `output := input`
- transform-only policy: `allow := true`, `output` reshapes fields
- filter + transform policy: both happen in one file

If a policy drops an object:

- later policies are skipped
- `ghrg policy test --show-dropped` can show the dropping policy and final visible object
- `ghrg policy trace` shows exactly where the chain stopped

## Local authoring workflow

The fastest policy loop is:

1. inspect the policy metadata
2. generate a schema sample if needed
3. run the policy locally
4. trace the chain when behavior is surprising
5. only then run against GitHub

### 1. Inspect metadata

```bash
ghrg policy inspect --policy examples/policies/project-summary.rego --format json
```

Use this to confirm:

- package name
- metadata source
- metadata name and description
- context declarations

For a built-in catalog of repository contexts and sample shapes:

```bash
ghrg contexts repos list
ghrg contexts repos show properties
```

### 2. Generate sample input

```bash
ghrg repos sample \
  --schema-only \
  --policy examples/policies/project-summary.rego \
  --output sample.json
```

This creates a sanitized repository-shaped input with policy-declared contexts already present.

You can also add explicit contexts:

```bash
ghrg repos sample \
  --schema-only \
  --policy examples/policies/project-summary.rego \
  --context files \
  --context languages
```

### 3. Run the policy locally

```bash
ghrg policy test \
  --policy examples/policies/filter-active.rego \
  --policy examples/policies/project-summary.rego \
  --input examples/inputs/repo.json \
  --format json
```

### 4. Trace the chain

```bash
ghrg policy trace \
  --policy examples/policies/filter-active.rego \
  --policy examples/policies/project-summary.rego \
  --input examples/inputs/repo.json
```

Use trace when you need:

- per-policy decisions
- metadata source visibility
- declared context visibility
- final output at each step

### 5. Run against GitHub

```bash
ghrg repos \
  --org acme \
  --policy examples/unarchived-repo-ownership-summary/filter-unarchived.rego \
  --policy examples/unarchived-repo-ownership-summary/repo-ownership-summary.rego \
  --format csv
```

## Authoring tips

- start with a cheap filter policy before expensive context-heavy policies
- prefer named contexts such as `repo_properties`
- keep `output` intentionally small once you reach reporting policies
- treat `meta` as optional debug data, not part of the user-facing report shape
- use `ghrg policy inspect` after metadata edits to catch sidecar or embedded metadata mistakes early

## Starter files to study

- `examples/policies/filter-active.rego`
- `examples/policies/project-summary.rego`
- `examples/policies/project-summary.ghrg.yaml`
- `examples/inputs/repo.json`
- `examples/README.md`

## Related docs

- Main overview: `README.md`
- Auth and setup: `docs/auth.md`
- Context reference: `docs/contexts.md`
- Examples index: `examples/README.md`
