# Examples

This directory keeps the starter examples small, runnable, and easy to adapt.

All property-based examples use the same public convention: `input.contexts.repo_properties`.

## Recommended starting points

- `examples/unarchived-repo-ownership-summary/`: keep unarchived repos, fetch ownership properties, and emit a CSV-friendly summary
- `examples/unarchived-stale-repo-ownership-summary/`: keep unarchived repos that have gone stale, then emit the same summary plus last update time
- `examples/policies/`: smaller building blocks for local policy authoring
- `examples/inputs/repo.json`: simple local input for `ghrg policy test`

If you are new to `ghrg`:

1. run `ghrg policy test` against one example folder
2. inspect the policy metadata and requested contexts
3. run the same policy chain with `ghrg repos` against a real org or repo

## End-to-end examples

### Unarchived Repo Ownership Summary

Use this when you want to:

- drop archived repositories
- read ownership properties from `repo_properties`
- export a compact ownership report

Quick local check:

```bash
ghrg policy test \
  --policy examples/unarchived-repo-ownership-summary/filter-unarchived.rego \
  --policy examples/unarchived-repo-ownership-summary/repo-ownership-summary.rego \
  --input examples/unarchived-repo-ownership-summary/sample-input-keep.json \
  --format json
```

Dropped-case check:

```bash
ghrg policy test \
  --policy examples/unarchived-repo-ownership-summary/filter-unarchived.rego \
  --policy examples/unarchived-repo-ownership-summary/repo-ownership-summary.rego \
  --input examples/unarchived-repo-ownership-summary/sample-input-drop.json \
  --show-dropped \
  --format json
```

Run against GitHub:

```bash
ghrg repos \
  --org acme \
  --policy examples/unarchived-repo-ownership-summary/filter-unarchived.rego \
  --policy examples/unarchived-repo-ownership-summary/repo-ownership-summary.rego \
  --format csv
```

See `examples/unarchived-repo-ownership-summary/README.md` for the full walkthrough.

### Unarchived Stale Repo Ownership Summary

Use this when you want to:

- drop archived repositories
- keep only repos whose `input.github.pushed_at` is stale
- export the ownership report with last update time

Quick local check:

```bash
ghrg policy test \
  --policy examples/unarchived-stale-repo-ownership-summary/filter-unarchived-stale.rego \
  --policy examples/unarchived-stale-repo-ownership-summary/repo-ownership-summary.rego \
  --input examples/unarchived-stale-repo-ownership-summary/sample-input-keep.json \
  --format json
```

Dropped-case check:

```bash
ghrg policy test \
  --policy examples/unarchived-stale-repo-ownership-summary/filter-unarchived-stale.rego \
  --policy examples/unarchived-stale-repo-ownership-summary/repo-ownership-summary.rego \
  --input examples/unarchived-stale-repo-ownership-summary/sample-input-drop.json \
  --show-dropped \
  --format json
```

Run against GitHub:

```bash
ghrg repos \
  --org acme \
  --policy examples/unarchived-stale-repo-ownership-summary/filter-unarchived-stale.rego \
  --policy examples/unarchived-stale-repo-ownership-summary/repo-ownership-summary.rego \
  --format csv
```

See `examples/unarchived-stale-repo-ownership-summary/README.md` for the full walkthrough.

## Policy authoring building blocks

For smaller local experiments:

- `examples/policies/filter-active.rego`: minimal embedded-metadata filter
- `examples/policies/project-summary.rego`: final projection policy with sidecar metadata
- `examples/policies/project-summary.ghrg.yaml`: requests the named `repo_properties` context
- `examples/inputs/repo.json`: local JSON input using the same `repo_properties` key

The starter projection uses the same title-cased report style as the end-to-end examples while keeping the input small enough for quick local iteration.

Local authoring loop:

```bash
ghrg policy inspect --policy examples/policies/project-summary.rego
ghrg policy test \
  --policy examples/policies/filter-active.rego \
  --policy examples/policies/project-summary.rego \
  --input examples/inputs/repo.json \
  --format json
```
