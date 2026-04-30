# Authentication and Setup

> WARNING: `ghrg` is currently AI-generated draft software. Verify this guide against the source and `ghrg --help` before depending on it.

This guide covers how `ghrg` discovers GitHub credentials, how to configure each supported auth path, and how to debug common setup failures.

## How auth lookup works

`ghrg` has two auth knobs:

- `--auth` selects a specific auth method
- `--auth-source` selects where secrets are loaded from

Current values:

- `--auth gh-cli`
- `--auth gh-app`
- `--auth-source env`
- `--auth-source secret-service`

If you do not pass `--auth`, `ghrg` runs in auto mode.

### Auto mode

Auto mode tries, in order:

1. personal token from `GITHUB_TOKEN`
2. personal token from `GH_TOKEN`
3. token from `gh auth token`
4. no GitHub credentials

That means:

- local-only commands like `ghrg policy test` still work without GitHub auth
- GitHub-backed commands like `ghrg repos` will fail later if no usable credentials are available or GitHub rate limits anonymous access
- GitHub App auth is never selected automatically; use `--auth gh-app` when you want it

### Inspect what `ghrg` will look up

Use `info` to inspect lookup order on your machine:

```bash
ghrg info --format json
```

This shows:

- selected auth method
- selected auth source
- personal token lookup keys
- GitHub App lookup keys
- the `gh auth token` fallback

## Personal token setup

Use a GitHub personal access token when you want the simplest setup for local use or CI.

### Bash or zsh

```bash
export GITHUB_TOKEN=ghp_xxx
ghrg repos --repo octo-org/api --format json
```

You can use `GH_TOKEN` instead:

```bash
export GH_TOKEN=ghp_xxx
ghrg repos --repo octo-org/api --format json
```

### Fish

```fish
set -x GITHUB_TOKEN ghp_xxx
ghrg repos --repo octo-org/api --format json
```

### GitHub Actions

```yaml
jobs:
  scan:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release -p ghrg
      - run: ./target/release/ghrg repos --org acme --format json
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

### Generic CI secret injection

```bash
export GITHUB_TOKEN="$CI_GITHUB_TOKEN"
ghrg repos --org acme --format csv --output repos.csv
```

## GitHub CLI auth

If you already use the GitHub CLI, `ghrg` can reuse that login through `gh auth token`.

Set it up locally:

```bash
gh auth login
gh auth status
ghrg repos --repo octo-org/api --format json
```

Force `ghrg` to use GitHub CLI auth explicitly:

```bash
ghrg --auth gh-cli repos --repo octo-org/api --format json
```

This is useful when you want to ignore any `GITHUB_TOKEN` or `GH_TOKEN` already present in the shell.

## GitHub App auth

Use GitHub App auth when you want installation-scoped access instead of a personal token.

Required values:

- `GHRG_GITHUB_APP_ID`
- `GHRG_GITHUB_INSTALLATION_ID`
- exactly one of:
  - `GHRG_GITHUB_PRIVATE_KEY`
  - `GHRG_GITHUB_PRIVATE_KEY_FILE`

### Private key from a file

```bash
export GHRG_GITHUB_APP_ID=12345
export GHRG_GITHUB_INSTALLATION_ID=67890
export GHRG_GITHUB_PRIVATE_KEY_FILE=/path/to/private-key.pem

ghrg --auth gh-app repos --org acme --format json
```

### Private key inline

```bash
export GHRG_GITHUB_APP_ID=12345
export GHRG_GITHUB_INSTALLATION_ID=67890
export GHRG_GITHUB_PRIVATE_KEY="$(cat /path/to/private-key.pem)"

ghrg --auth gh-app repos --org acme --format json
```

### CI example

```bash
export GHRG_GITHUB_APP_ID="$CI_GITHUB_APP_ID"
export GHRG_GITHUB_INSTALLATION_ID="$CI_GITHUB_INSTALLATION_ID"
export GHRG_GITHUB_PRIVATE_KEY="$CI_GITHUB_PRIVATE_KEY"

./target/release/ghrg --auth gh-app repos --org acme --format json
```

Notes:

- `ghrg` expects an RSA private key in PEM or DER format
- do not set both `GHRG_GITHUB_PRIVATE_KEY` and `GHRG_GITHUB_PRIVATE_KEY_FILE`
- auto mode does not pick GitHub App auth; you must pass `--auth gh-app`

## Secret Service mode

Use `--auth-source secret-service` to read credentials from the local Secret Service backend instead of environment variables.

Availability:

- supported in Linux GNU builds compiled with the `secret-service` feature
- not included in the portable official release binaries
- not available in musl/static Linux builds

Service name:

- `ghrg`

Supported entry names:

- `github-token`
- `github-app-id`
- `github-installation-id`
- `github-private-key`

Examples:

```bash
ghrg --auth-source secret-service repos --repo octo-org/api --format json
```

```bash
ghrg --auth gh-app --auth-source secret-service repos --org acme --format json
```

Behavior matches env mode:

- auto mode checks the Secret Service personal token first, then falls back to `gh auth token`
- GitHub App credentials are only used when you pass `--auth gh-app`

If you see an error saying Secret Service is not supported by the current build, switch to `GITHUB_TOKEN` / `GH_TOKEN` or use a Linux GNU build that enables the `secret-service` feature.

## Setup recipes

### Fast local setup with a PAT

```bash
export GITHUB_TOKEN=ghp_xxx
ghrg info --format json
ghrg repos --repo octo-org/api --format json
```

### Fast local setup with GitHub CLI

```bash
gh auth login
gh auth status
ghrg info
ghrg repos --repo octo-org/api --format json
```

### CI setup for a policy-based scan

```bash
cargo build --release -p ghrg
./target/release/ghrg repos \
  --org acme \
  --policy examples/unarchived-repo-ownership-summary/filter-unarchived.rego \
  --policy examples/unarchived-repo-ownership-summary/repo-ownership-summary.rego \
  --format csv \
  --output repos.csv
```

Provide one of the supported auth methods through CI secrets.

## Troubleshooting

### `failed to acquire GitHub auth token`

Common causes:

- `gh` is not installed
- `gh auth login` has not been run
- `gh auth token` returns an empty value
- your shell does not have `GITHUB_TOKEN` or `GH_TOKEN`

Checks:

```bash
gh auth status
gh auth token
ghrg info --format json
```

### Missing GitHub App values

If `--auth gh-app` fails immediately, confirm all required variables are present:

```bash
env | grep '^GHRG_GITHUB_'
```

You need:

- `GHRG_GITHUB_APP_ID`
- `GHRG_GITHUB_INSTALLATION_ID`
- one of `GHRG_GITHUB_PRIVATE_KEY` or `GHRG_GITHUB_PRIVATE_KEY_FILE`

### Both private key variables are set

`ghrg` rejects this configuration. Keep exactly one:

- `GHRG_GITHUB_PRIVATE_KEY`
- `GHRG_GITHUB_PRIVATE_KEY_FILE`

### Failed to read GitHub App private key file

Checks:

- path exists
- current user can read it
- file contains the expected private key

Quick check:

```bash
ls -l /path/to/private-key.pem
```

### Failed to parse GitHub App private key

Use a valid RSA private key in PEM or DER format. Common issues are:

- wrong secret pasted into the variable
- key was truncated by CI secret handling
- non-RSA key material

### Secret Service entry missing

If you use `--auth-source secret-service`, confirm the `ghrg` service contains the expected entry names listed earlier in this guide.

### Auth looks right, but GitHub requests still fail

The token may be valid but missing the permissions needed for the repositories or API endpoints you are scanning.

Good checks:

- try `ghrg repos --repo owner/name --format raw` on a single known repo first
- compare behavior with `gh api` or `gh auth status`
- confirm repository visibility and installation scope for GitHub App auth

## Related docs

- Main overview: `README.md`
- Starter policy examples: `examples/policies/`
- End-to-end ownership example: `examples/unarchived-repo-ownership-summary/README.md`
