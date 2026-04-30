# Unarchived Stale Repo Ownership Summary

This example shows a two-stage policy chain:

1. keep only repositories that are unarchived and stale
2. read ownership properties from `input.contexts.repo_properties`
3. emit the ownership report plus the last push time

Visible output fields:

- `Name`
- `Team`
- `CodeOwner`
- `Public`
- `Last Update`

The staleness filter reads `input.github.pushed_at` from the base repository payload, so it does not need an extra commit-history context fetch.

Files in this folder:

- `filter-unarchived-stale.rego`: first-stage filter using `input.github.pushed_at`
- `repo-ownership-summary.rego`: final projection policy
- `repo-ownership-summary.ghrg.yaml`: requests the named `repo_properties` context
- `sample-input-keep.json`: local input that should pass the filter
- `sample-input-drop.json`: local input that should be dropped
- `generated-sample.json`: schema-style sample including `github`

Run against GitHub:

```bash
ghrg repos \
  --org acme \
  --policy examples/unarchived-stale-repo-ownership-summary/filter-unarchived-stale.rego \
  --policy examples/unarchived-stale-repo-ownership-summary/repo-ownership-summary.rego \
  --format csv
```

Use `--format json` for structured output instead.

Test the kept case locally:

```bash
ghrg policy test \
  --policy examples/unarchived-stale-repo-ownership-summary/filter-unarchived-stale.rego \
  --policy examples/unarchived-stale-repo-ownership-summary/repo-ownership-summary.rego \
  --input examples/unarchived-stale-repo-ownership-summary/sample-input-keep.json \
  --format json
```

Test the dropped case and show why it was rejected:

```bash
ghrg policy test \
  --policy examples/unarchived-stale-repo-ownership-summary/filter-unarchived-stale.rego \
  --policy examples/unarchived-stale-repo-ownership-summary/repo-ownership-summary.rego \
  --input examples/unarchived-stale-repo-ownership-summary/sample-input-drop.json \
  --show-dropped \
  --format json
```

Generate a fresh schema sample with the requested contexts included:

```bash
ghrg repos sample \
  --schema-only \
  --policy examples/unarchived-stale-repo-ownership-summary/repo-ownership-summary.rego \
  --output examples/unarchived-stale-repo-ownership-summary/generated-sample.json
```

Both the sample inputs and the generated sample use the same named-context shape expected by the projection policy: `input.contexts.repo_properties`.

Expected output shape:

```json
{
  "CodeOwner": "@example/platform",
  "Last Update": "2025-01-01T00:00:00Z",
  "Name": "api",
  "Public": true,
  "Team": "platform"
}
```
