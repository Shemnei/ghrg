# Context Reference

This page documents the repository context kinds that `ghrg` can attach under `input.contexts` during policy evaluation.

Use contexts when a policy needs data beyond the base repository object.

You can also inspect the same information directly from the CLI:

```bash
ghrg contexts repos list
ghrg contexts repos show properties
ghrg contexts repos show files --format json
```

## How contexts show up in policy input

Contexts are attached under `input.contexts`.

There are two patterns:

- unnamed context: key defaults to the context kind, such as `input.contexts.languages`
- named context: key uses the declared `name`, such as `input.contexts.repo_properties`

The public docs prefer named contexts when the key is part of a long-lived policy contract.

Example:

```yaml
contexts:
  - name: repo_properties
    type: properties
    names:
      - Team
      - CodeOwner
```

Then in Rego:

```rego
input.contexts.repo_properties.Team
```

## Choosing context shapes

- use `properties` for custom ownership or governance metadata
- use `languages` for language mix checks
- use `branches` for branch policy checks
- use `commits` for recent activity or author/path checks
- use `files` for file presence or path-based checks
- use `contributors` for contributor/activity summaries
- use `workflow_runs` for recent GitHub Actions health or recency checks

## `properties`

Purpose:

- fetch selected custom repository properties by name

Config fields:

- required: `names`
- optional: `name`

Rules:

- `names` must contain at least one non-empty string

Example declaration:

```yaml
contexts:
  - name: repo_properties
    type: properties
    names:
      - Team
      - CodeOwner
```

Sample shape:

```json
{
  "repo_properties": {
    "CodeOwner": "@example/platform",
    "Team": "platform"
  }
}
```

Policy usage:

```rego
input.contexts.repo_properties.Team
```

Performance notes:

- relatively cheap compared with file or commit-heavy contexts
- good early enrichment context for reporting policies

## `languages`

Purpose:

- fetch the repository language byte breakdown

Config fields:

- optional: `name`
- no extra required parameters

Example declaration:

```yaml
contexts:
  - type: languages
```
or:
```yaml
contexts:
  - name: repo_languages
    type: languages
```

Sample shape:

```json
{
  "languages": {
    "Dockerfile": 88,
    "Rust": 18234,
    "Shell": 312
  }
}
```

Policy usage:

```rego
input.contexts.languages.Rust
```

Performance notes:

- usually cheap
- useful for rough composition checks without walking files

## `branches`

Purpose:

- fetch repository branches with optional protection filtering

Config fields:

- optional: `name`
- optional: `limit`
- optional: `protected`

Rules:

- `limit` must be positive
- live requests are clamped to 100

Example declaration:

```yaml
contexts:
  - name: protected_branches
    type: branches
    limit: 5
    protected: true
```

Sample shape:

```json
{
  "protected_branches": [
    {
      "name": "main",
      "protected": true,
      "sha": "00000000000000000000000000000000000000c9",
      "url": "https://api.github.com/repos/example-org/example-repo/branches/main"
    }
  ]
}
```

Policy usage:

```rego
count(input.contexts.protected_branches) > 0
```

Performance notes:

- moderate cost depending on limit
- keep `limit` narrow if you only need a few branches

## `commits`

Purpose:

- fetch recent commits, optionally filtered by path, author, or ref

Config fields:

- optional: `name`
- optional: `limit`
- optional: `path`
- optional: `author`
- optional: `ref`

Rules:

- `limit` must be positive
- `path`, `author`, and `ref` must be non-empty when present
- live requests are clamped to 100

Example declaration:

```yaml
contexts:
  - name: recent_src_commits
    type: commits
    limit: 3
    path: src/
    ref: main
```

Sample shape:

```json
{
  "recent_src_commits": [
    {
      "author": "octocat",
      "message": "Sample commit 1 for example-org/example-repo",
      "path": "src/",
      "ref": "main",
      "sha": "0000000000000000000000000000000000000001"
    }
  ]
}
```

Policy usage:

```rego
some commit in input.contexts.recent_src_commits
commit.author == "octocat"
```

Performance notes:

- more expensive than `properties` or `languages`
- path and author filters help keep the result focused
- best used after cheap filters when scanning large orgs

## `files`

Purpose:

- fetch repository file entries, optionally filtered by glob and ref

Config fields:

- optional: `name`
- optional: `glob`
- optional: `limit`
- optional: `ref`

Rules:

- `glob` and `ref` must be non-empty when present
- `limit` must be positive
- live requests are clamped to 500
- when `ref` is omitted, `ghrg` defaults to the repo default branch

Example declaration:

```yaml
contexts:
  - name: workflow_files
    type: files
    glob: .github/workflows/*.yml
    limit: 20
    ref: main
```

Sample shape:

```json
{
  "workflow_files": [
    {
      "glob": ".github/workflows/*.yml",
      "mode": "100644",
      "name": "check-1.yml",
      "path": ".github/workflows/check-1.yml",
      "reference": "main",
      "sha": "0000000000000000000000000000000000000065",
      "size": 200,
      "type": "blob"
    }
  ]
}
```

Policy usage:

```rego
count(input.contexts.workflow_files) > 0
```

Performance notes:

- often one of the more expensive contexts
- always narrow by `glob` and set a realistic `limit`
- avoid using this as a first-stage check across very large scans when a cheaper signal exists

## `contributors`

Purpose:

- fetch contributor summaries, with optional anonymous entries

Config fields:

- optional: `name`
- optional: `limit`
- optional: `anonymous`

Rules:

- `limit` must be positive
- live requests are clamped to 100

Example declaration:

```yaml
contexts:
  - name: top_contributors
    type: contributors
    limit: 10
    anonymous: false
```

Sample shape:

```json
{
  "top_contributors": [
    {
      "anonymous": false,
      "avatar_url": "https://avatars.githubusercontent.com/u/1000",
      "contributions": 20,
      "email": null,
      "html_url": "https://github.com/contributor-1",
      "id": 1000,
      "login": "contributor-1",
      "type": "User"
    }
  ]
}
```

Policy usage:

```rego
count(input.contexts.top_contributors) >= 3
```

Performance notes:

- moderate cost
- limit the result set if you only need a summary threshold

## `workflow_runs`

Purpose:

- fetch recent GitHub Actions workflow runs, optionally filtered by branch, event, status, or actor

Config fields:

- optional: `name`
- optional: `limit`
- optional: `branch`
- optional: `event`
- optional: `status`
- optional: `actor`

Rules:

- `limit` must be positive
- `branch`, `event`, `status`, and `actor` must be non-empty when present
- live requests clamp `limit` to 100

Example declaration:

```yaml
contexts:
  - name: recent_workflow_runs
    type: workflow_runs
    limit: 5
    branch: main
    status: completed
```

Sample shape:

```json
{
  "recent_workflow_runs": [
    {
      "id": 9000,
      "name": "CI",
      "event": "push",
      "status": "completed",
      "conclusion": "success",
      "head_branch": "main",
      "head_sha": "000000000000000000000000000000000000012d",
      "run_number": 41,
      "run_attempt": 1,
      "actor_login": "octocat",
      "html_url": "https://github.com/example-org/example-repo/actions/runs/9000",
      "created_at": "2026-01-01T12:00:00Z",
      "updated_at": "2026-01-01T12:05:00Z"
    }
  ]
}
```

Policy usage:

```rego
some run in input.contexts.recent_workflow_runs
run.conclusion == "success"
```

Performance notes:

- moderate cost
- filter by branch, event, or status if you only need recent CI health

## Explicit contexts in `repos sample`

You can generate schema samples with explicit context kinds even without policy metadata:

```bash
ghrg repos sample --schema-only --context files --context languages
```

These explicit contexts use the kind as the input key, such as:

- `input.contexts.files`
- `input.contexts.languages`

For public starter policies, prefer named metadata-based contexts where the key matters.

## Suggested usage order

When building a policy chain for large scans:

1. start with no-context or low-cost filters
2. add `properties` or `languages` if needed
3. add `branches`, `contributors`, `commits`, or `files` later in the chain
4. keep the final report policy narrow

## Related docs

- Policy authoring: `docs/policy-authoring.md`
- Main overview: `README.md`
- Examples index: `examples/README.md`
