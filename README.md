# Gist Config Sync (gcs)

Sync project config files to and from GitHub gists.

Tracks which files belong to a project in a YAML manifest stored
inside the gist itself. After the first sync, running `gcs` with no
arguments reads the manifest and does the right thing automatically.

## Install

```
cargo install --path .
```

## Usage

```
gcs [options] [command] [files...]
```

### Commands

| Command | Description |
|---------|-------------|
| `sync` | Sync files with the gist (default) |
| `add [--platform] <files>` | Add files to the tracked set and push them |
| `remove <files>` | Remove files from the tracked set and delete from gist |
| `delete` | Delete the entire config gist |

### Options

| Option | Description |
|--------|-------------|
| `--name <NAME>` | Gist name prefix (default: basename of project root) |
| `--root <DIR>` | Root directory for relative paths (default: git root or cwd) |
| `--private` | Create the gist as secret instead of public |

## Examples

Track some config files for the first time:

```
gcs add .claude/CLAUDE.md .vscode/settings.json
```

Sync on another machine (reads the manifest, pulls newer files):

```
gcs
```

Add another file later:

```
gcs add .vscode/launch.json
```

Add a file that only applies to the current platform:

```
gcs add --platform .vscode/cmake-variants.yaml
```

The manifest records the platform automatically. When syncing on a
different OS, platform-specific files are skipped.

Stop tracking a file:

```
gcs remove .vscode/launch.json
```

Delete the gist entirely:

```
gcs delete
```

## Secret scanning

Before pushing files to a gist, `gcs` scans them for potential secrets
(API keys, tokens, passwords, etc.) and aborts if any are found.

## How it works

Each project gets a single gist identified by the description
`"<project> config-sync"`. Files are stored with underscores replacing
path separators (`zlib-ng_.vscode_settings.json`). A manifest file
(`.zlib-ng-config-sync.yaml`) in the gist records the mapping between
local paths and gist filenames.

Syncing compares the newest local file modification time against the
gist's `updated_at` timestamp — whichever side is newer wins.

## Authentication

Uses `gh auth token` if the GitHub CLI is installed, otherwise falls
back to the `GITHUB_TOKEN` or `GH_TOKEN` environment variables.

## License

MIT
