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
| `add [--platform] <files>` | Add files to the tracked set and push them |
| `cleanup` | Remove gist files not listed in the manifest |
| `delete` | Delete the entire config gist |
| `open` | Open the project's gist in a web browser |
| `remove [--platform] <files>` | Remove files from the tracked set and delete from gist |
| `restore <files>` | Overwrite local files with the gist version, ignoring mtime |
| `status [files]` | Report what sync would do without making any changes |
| `sync` | Sync files with the gist (default) |

### Options

| Option | Description |
|--------|-------------|
| `--name <NAME>` | Gist name prefix (default: basename of project root) |
| `--root <DIR>` | Root directory for relative paths (default: git root or cwd) |
| `--private` | Create the gist as secret instead of public |
| `-V`, `--version` | Print version information |

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

The same path may carry a separate variant per platform. Run the same
`add --platform` on each OS and each keeps its own content in the gist;
sync only ever touches the variant for the platform it runs on.

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
path separators (`zlib-ng_.vscode_settings.json`); platform-specific
files tag the platform before the extension
(`zlib-ng_.vscode_settings[macos].json`) so variants of one path
never collide. A manifest file
(`.zlib-ng-config-sync.yaml`) in the gist records the mapping between
local paths and gist filenames.

Syncing compares the newest local file modification time against the
gist's `updated_at` timestamp — whichever side is newer wins.

## Authentication

Uses `gh auth token` if the GitHub CLI is installed, otherwise falls
back to the `GITHUB_TOKEN` or `GH_TOKEN` environment variables.

## License

MIT
