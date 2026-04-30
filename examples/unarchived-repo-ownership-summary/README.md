# Unarchived Repo Ownership Summary

This example shows a two-stage policy chain:

1. keep only unarchived repositories
2. read ownership properties from `input.contexts.repo_properties`
3. emit a compact ownership report

Visible output fields:

- `Name`
- `Team`
- `CodeOwner`
- `Public`

Files in this folder:

- `filter-unarchived.rego`: first-stage filter that drops archived repositories
- `repo-ownership-summary.rego`: final projection policy
- `repo-ownership-summary.ghrg.yaml`: requests the named `repo_properties` context
- `sample-input-keep.json`: local input that should pass the filter
- `sample-input-drop.json`: local input that should be dropped
- `generated-sample.json`: schema-style sample for policy authoring

Run against GitHub:

```bash
ghrg repos \
  --org acme \
  --policy examples/unarchived-repo-ownership-summary/filter-unarchived.rego \
  --policy examples/unarchived-repo-ownership-summary/repo-ownership-summary.rego \
  --format csv
```

Use `--format json` for structured output instead.

Test the kept case locally:

```bash
ghrg policy test \
  --policy examples/unarchived-repo-ownership-summary/filter-unarchived.rego \
  --policy examples/unarchived-repo-ownership-summary/repo-ownership-summary.rego \
  --input examples/unarchived-repo-ownership-summary/sample-input-keep.json \
  --format json
```

Test the dropped case and show why it was rejected:

```bash
ghrg policy test \
  --policy examples/unarchived-repo-ownership-summary/filter-unarchived.rego \
  --policy examples/unarchived-repo-ownership-summary/repo-ownership-summary.rego \
  --input examples/unarchived-repo-ownership-summary/sample-input-drop.json \
  --show-dropped \
  --format json
```

Generate a fresh schema sample with the requested contexts included:

```bash
ghrg repos sample \
  --schema-only \
  --policy examples/unarchived-repo-ownership-summary/repo-ownership-summary.rego \
  --output examples/unarchived-repo-ownership-summary/generated-sample.json
```

Both the sample inputs and the generated sample use the same named-context shape expected by the projection policy: `input.contexts.repo_properties`.

Expected output shape:

```json
{
  "CodeOwner": "@example/platform",
  "Name": "api",
  "Public": true,
  "Team": "platform"
}
```
