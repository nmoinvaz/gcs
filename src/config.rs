use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

pub struct ProjectConfig {
    pub root: PathBuf,
    pub name: String,
    pub public: bool,
}

impl ProjectConfig {
    pub fn new(name: Option<String>, root: Option<PathBuf>, private: bool) -> Result<Self> {
        let root = match root {
            Some(r) => r,
            None => git_toplevel().unwrap_or_else(|| std::env::current_dir().unwrap()),
        };
        let name = name.unwrap_or_else(|| {
            root.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        });
        Ok(Self {
            root,
            name,
            public: !private,
        })
    }

    pub fn gist_description(&self) -> String {
        format!("{} config-sync", self.name)
    }

    pub fn manifest_name(&self) -> String {
        format!(".{}-config-sync.yaml", self.name)
    }

    /// Convert a local path to a gist-safe filename using _ as directory separator.
    pub fn gist_filename(&self, path: &str) -> String {
        let normalized = path.strip_prefix("./").unwrap_or(path);
        format!("{}_{}", self.name, normalized.replace('/', "_"))
    }

    pub fn repo_url(&self) -> Option<String> {
        let output = Command::new("git")
            .args(["remote", "get-url", "origin"])
            .current_dir(&self.root)
            .output()
            .ok()?;
        if output.status.success() {
            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if url.is_empty() {
                None
            } else {
                Some(url)
            }
        } else {
            None
        }
    }
}

fn git_toplevel() -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Some(PathBuf::from(path))
    } else {
        None
    }
}

pub fn get_github_token() -> Result<String> {
    // Try gh auth token first.
    if let Ok(output) = Command::new("gh").args(["auth", "token"]).output() {
        if output.status.success() {
            let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !token.is_empty() {
                return Ok(token);
            }
        }
    }

    // Fall back to environment variables.
    for var in ["GITHUB_TOKEN", "GH_TOKEN"] {
        if let Ok(token) = std::env::var(var) {
            if !token.is_empty() {
                return Ok(token);
            }
        }
    }

    anyhow::bail!("No GitHub token found. Run `gh auth login` or set GITHUB_TOKEN.")
}
