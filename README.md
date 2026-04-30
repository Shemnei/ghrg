> [!WARNING]
> This project is currently AI-generated slop.
> Treat the code, docs, examples, and release-readiness claims as unverified until human review.
> See `DISCLAIMER.md` for the project-wide note.

# ghrg

```text
> ghrg repos --auth=gh-cli --user Shemnei --policy examples/unarchived-stale-repo-ownership-summary/filter-unarchived-stale.rego --policy examples/unarchived-stale-repo-ownership-summary/repo-ownership-summary.rego
done Prepared repository scan with 2 policies
done Loaded 61 repositories
done Scanned 61 repositories: 51 kept, 10 dropped, 0 failed
Repositories
51 records

╭───────────┬──────────────────────┬───────────────────────────────────┬────────┬──────╮
│ CodeOwner ┆ Last Update          ┆ Name                              ┆ Public ┆ Team │
╞═══════════╪══════════════════════╪═══════════════════════════════════╪════════╪══════╡
│ null      ┆ 2022-12-06T14:29:17Z ┆ aoc2022                           ┆ true   ┆ null │
├╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌┤
│ null      ┆ 2017-12-22T15:10:33Z ┆ AoC_2017                          ┆ true   ┆ null │
├╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌┤
│ null      ┆ 2018-12-08T11:10:46Z ┆ AoC_2018                          ┆ true   ┆ null │
├╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌┤
│ null      ┆ 2024-08-04T12:14:04Z ┆ ArchiveBox                        ┆ true   ┆ null │
├╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌┤
│ null      ┆ 2021-03-31T17:09:25Z ┆ brnfk                             ┆ true   ┆ null │
├╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌┤
│ null      ┆ 2022-02-23T23:15:24Z ┆ cards                             ┆ true   ┆ null │
├╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌┤
│ null      ┆ 2021-09-16T20:17:32Z ┆ carob                             ┆ true   ┆ null │

...
```

`ghrg` is a GitHub repository governance CLI for scanning repositories, enriching them with GitHub context, and applying Rego-based policies to filter or reshape the final output.

It is for platform teams, security teams, and repository owners who want a repeatable way to answer questions like:

- Which repos are active and unarchived?
- Which repos are missing expected ownership properties?
- Which repos should be reported, grouped, or exported as CSV?

`ghrg` is designed for a short first-run path:

1. install or build the CLI
2. point it at GitHub auth
3. run `ghrg info`
4. run `ghrg policy test`
5. run `ghrg repos`

## What it does

`ghrg` works in three layers:

- `info` shows runtime, cache, logging, and auth lookup details.
- `policy` lets you inspect, test, and trace policies locally with JSON input.
- `repos` scans GitHub repositories, fetches requested contexts, applies policies, and renders the visible results.

Policies are written in Rego and can declare metadata plus requested contexts such as repository properties, languages, branches, commits, files, contributors, or workflow runs.

## Install and build

Build a release binary:

```bash
cargo build --release -p ghrg
./target/release/ghrg --help
```

Or install it into Cargo's bin directory:

```bash
cargo install --path crates/ghrg-cli
ghrg --help
```

For local iteration during development:

```bash
cargo run -p ghrg -- --help
```

## Authentication

`ghrg` supports a few GitHub auth paths.

Default auto mode:

- checks `GITHUB_TOKEN`
- checks `GH_TOKEN`
- falls back to `gh auth token`
- if none are available, commands that do not need GitHub access can still run locally

Personal token via environment:

```bash
export GITHUB_TOKEN=ghp_xxx
ghrg info
```

GitHub CLI token:

```bash
gh auth login
ghrg info
```

GitHub App auth:

```bash
export GHRG_GITHUB_APP_ID=12345
export GHRG_GITHUB_INSTALLATION_ID=67890
export GHRG_GITHUB_PRIVATE_KEY_FILE=/path/to/private-key.pem

ghrg --auth gh-app info
```

You can also provide the private key inline with `GHRG_GITHUB_PRIVATE_KEY` instead of `GHRG_GITHUB_PRIVATE_KEY_FILE`.

Secret Service lookup is only available in Linux GNU builds compiled with the `secret-service` feature.

Official release binaries use a portable feature set and do not include Secret Service support.

Use `ghrg info --format json` to see exactly which auth sources the CLI will check on your machine.

For full setup, CI examples, Secret Service details, and troubleshooting, see `docs/auth.md`.

## 5-minute quickstart

Confirm the CLI is wired up:

```bash
ghrg info
```

Test a policy locally with bundled example input:

```bash
ghrg policy test \
  --policy examples/policies/filter-active.rego \
  --policy examples/policies/project-summary.rego \
  --input examples/inputs/repo.json \
  --format json
```

Inspect the policy metadata and requested contexts:

```bash
ghrg policy inspect \
  --policy examples/policies/project-summary.rego
```

Generate a schema-style sample input for policy authoring:

```bash
ghrg repos sample \
  --schema-only \
  --policy examples/policies/project-summary.rego
```

Scan a real organization and export visible fields as CSV:

```bash
ghrg repos \
  --org acme \
  --policy examples/unarchived-repo-ownership-summary/filter-unarchived.rego \
  --policy examples/unarchived-repo-ownership-summary/repo-ownership-summary.rego \
  --format csv
```

## Core command groups

### `info`

Use `info` first when you want to confirm local setup.

- shows auth method and auth lookup order
- shows cache settings and current cache size
- shows log file location, version, platform, and execution id
- supports `--format pretty` and `--format json`

Example:

```bash
ghrg info --format json
```

### `policy`

Use `policy` to iterate locally before touching GitHub.

- `ghrg policy inspect` loads a policy and prints package, metadata source, and contexts
- `ghrg policy test` evaluates one or more policies against local JSON input and renders the visible report shape
- `ghrg policy trace` shows step-by-step evaluation details across a policy chain, including metadata and requested contexts

Examples:

```bash
ghrg policy inspect --policy examples/policies/project-summary.rego
```

Browse supported repository contexts from the CLI:

```bash
ghrg contexts repos list
ghrg contexts repos show files --format json
```

```bash
ghrg policy trace \
  --policy examples/policies/filter-active.rego \
  --policy examples/policies/project-summary.rego \
  --input examples/inputs/repo.json
```

### `repos`

Use `repos` to scan GitHub repositories and optionally apply policies.

Supported scopes:

- `--org <org>`
- `--user <user>`
- `--owner <owner>`
- `--repo <owner/name>`

Common flags:

- `--policy <path>` to apply one or more policies in order
- `--format pretty|json|csv|raw`
- `--group-by <field>` and `--sort-by <field>` for pretty output
- `--limit <n>` to cap repository listing
- `--concurrency <n>` to control parallel fetch/evaluation
- `--output <path>` to write results to a file

Examples:

```bash
ghrg repos --repo octo-org/api --format json
```

```bash
ghrg repos \
  --org acme \
  --policy examples/unarchived-stale-repo-ownership-summary/filter-unarchived-stale.rego \
  --policy examples/unarchived-stale-repo-ownership-summary/repo-ownership-summary.rego \
  --format pretty \
  --group-by Team
```

## Output formats

`ghrg` separates the final visible output from policy metadata.

- `pretty` is human-friendly terminal output
- `json` emits the final visible object or array
- `csv` exports scalar repository fields for spreadsheet/reporting workflows
- `raw` keeps the command envelope and metadata for debugging and traceability

In practice:

- use `json` when piping into `jq` or other automation
- use `csv` for ownership reports and ad hoc exports
- use `raw` when you want policy meta alongside visible values

## Common workflows

Local authoring loop:

```bash
ghrg policy inspect --policy path/to/policy.rego
ghrg repos sample --schema-only --policy path/to/policy.rego --output sample.json
ghrg policy test --policy path/to/policy.rego --input sample.json
ghrg policy trace --policy path/to/policy.rego --input sample.json
```

Organization report:

```bash
ghrg repos \
  --org acme \
  --policy path/to/filter.rego \
  --policy path/to/report.rego \
  --format csv \
  --output repos.csv
```

Repo-level debugging:

```bash
ghrg repos --repo acme/api --format raw
```

## Examples and docs

- Starter policies: `examples/policies/`
- Examples index: `examples/README.md`
- Example input: `examples/inputs/repo.json`
- Starter report example: `examples/policies/project-summary.rego`
- Project disclaimer: `DISCLAIMER.md`
- Auth and setup guide: `docs/auth.md`
- Policy authoring guide: `docs/policy-authoring.md`
- Context reference: `docs/contexts.md`
- CLI context browser: `ghrg contexts repos list`
- Ownership summary example: `examples/unarchived-repo-ownership-summary/README.md`
- Stale repo ownership example: `examples/unarchived-stale-repo-ownership-summary/README.md`

Today the best deeper references are the example directories plus command help:

```bash
ghrg --help
ghrg policy --help
ghrg repos --help
```
