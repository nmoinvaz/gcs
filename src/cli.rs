use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "gcs", about = "Sync project config files to/from GitHub gists")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Gist name prefix (default: basename of project root)
    #[arg(long, global = true)]
    pub name: Option<String>,

    /// Root directory for relative paths (default: git root or cwd)
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,

    /// Create the gist as secret instead of public
    #[arg(long, global = true)]
    pub private: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Sync files with the gist (default when no command given)
    Sync {
        /// Files to sync, relative to root
        files: Vec<String>,
    },
    /// Add files to the tracked set and push them
    Add {
        /// Mark these files as specific to the current platform
        #[arg(long)]
        platform: bool,

        /// Files to add, relative to root
        files: Vec<String>,
    },
    /// Remove files from the tracked set and delete from gist
    Remove {
        /// Files to remove, relative to root
        files: Vec<String>,
    },
    /// Remove gist files not listed in the manifest
    Cleanup,
    /// Delete the entire config gist
    Delete,
}
