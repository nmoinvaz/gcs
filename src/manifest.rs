use serde::{Deserialize, Serialize};

use crate::config::ProjectConfig;

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    pub url: String,
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ManifestFile {
    pub path: String,
    pub gist: String,
}

impl Manifest {
    pub fn new(config: &ProjectConfig, paths: &[String]) -> Self {
        let files = paths
            .iter()
            .map(|p| ManifestFile {
                path: p.clone(),
                gist: config.gist_filename(p),
            })
            .collect();
        Self {
            name: config.name.clone(),
            description: "Configuration files synced by config-sync".to_string(),
            repo: config.repo_url(),
            url: "https://github.com/nmoinvaz/gcs".to_string(),
            files,
        }
    }

    pub fn to_yaml(&self) -> String {
        serde_yaml::to_string(self).unwrap_or_default()
    }

    pub fn from_yaml(yaml: &str) -> Option<Self> {
        serde_yaml::from_str(yaml).ok()
    }

    pub fn paths(&self) -> Vec<String> {
        self.files.iter().map(|f| f.path.clone()).collect()
    }
}
