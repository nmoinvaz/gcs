use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct GistClient {
    client: Client,
    token: String,
}

impl GistClient {
    pub fn new(token: String) -> Result<Self> {
        let client = Client::builder()
            .user_agent("gcs")
            .build()
            .context("Failed to create HTTP client")?;
        Ok(Self { client, token })
    }

    /// Search for a gist by its description, returning the gist ID if found.
    pub fn find_gist(&self, description: &str) -> Result<Option<String>> {
        let mut page = 1u32;
        loop {
            let resp: Vec<Value> = self
                .client
                .get("https://api.github.com/gists")
                .bearer_auth(&self.token)
                .query(&[("per_page", "100"), ("page", &page.to_string())])
                .send()
                .context("Failed to list gists")?
                .error_for_status()
                .context("GitHub API error listing gists")?
                .json()
                .context("Failed to parse gist list")?;

            if resp.is_empty() {
                return Ok(None);
            }

            for gist in &resp {
                if let Some(desc) = gist["description"].as_str() {
                    if desc == description {
                        if let Some(id) = gist["id"].as_str() {
                            return Ok(Some(id.to_string()));
                        }
                    }
                }
            }

            page += 1;
        }
    }

    /// Fetch a gist by ID.
    pub fn get_gist(&self, id: &str) -> Result<Value> {
        self.client
            .get(format!("https://api.github.com/gists/{id}"))
            .bearer_auth(&self.token)
            .send()
            .context("Failed to fetch gist")?
            .error_for_status()
            .context("GitHub API error fetching gist")?
            .json()
            .context("Failed to parse gist response")
    }

    /// Get the updated_at timestamp from a gist.
    pub fn get_updated_at(&self, gist: &Value) -> Option<DateTime<Utc>> {
        gist["updated_at"]
            .as_str()
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
    }

    /// Extract a file's content from a gist response.
    pub fn get_file_content<'a>(&self, gist: &'a Value, filename: &str) -> Option<&'a str> {
        gist["files"][filename]["content"].as_str()
    }

    /// List all filenames in a gist response.
    pub fn get_file_names(&self, gist: &Value) -> Vec<String> {
        gist["files"]
            .as_object()
            .map(|files| files.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Create a new gist with the given files. Returns the gist ID.
    pub fn create_gist(
        &self,
        description: &str,
        public: bool,
        files: &HashMap<String, String>,
    ) -> Result<String> {
        let mut file_map = serde_json::Map::new();
        for (name, content) in files {
            file_map.insert(name.clone(), json!({"content": content}));
        }

        let body = json!({
            "description": description,
            "public": public,
            "files": file_map,
        });

        let resp: Value = self
            .client
            .post("https://api.github.com/gists")
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .context("Failed to create gist")?
            .error_for_status()
            .context("GitHub API error creating gist")?
            .json()
            .context("Failed to parse create gist response")?;

        resp["id"]
            .as_str()
            .map(|s| s.to_string())
            .context("Gist ID not found in response")
    }

    /// Update gist files. Use None as the value to delete a file.
    pub fn update_files(&self, id: &str, files: &HashMap<String, Option<String>>) -> Result<()> {
        let mut file_map = serde_json::Map::new();
        for (name, content) in files {
            match content {
                Some(c) => {
                    file_map.insert(name.clone(), json!({"content": c}));
                }
                None => {
                    file_map.insert(name.clone(), Value::Null);
                }
            }
        }

        let body = json!({"files": file_map});

        self.client
            .patch(format!("https://api.github.com/gists/{id}"))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .context("Failed to update gist")?
            .error_for_status()
            .context("GitHub API error updating gist")?;

        Ok(())
    }

    /// Delete a gist entirely.
    pub fn delete_gist(&self, id: &str) -> Result<()> {
        self.client
            .delete(format!("https://api.github.com/gists/{id}"))
            .bearer_auth(&self.token)
            .send()
            .context("Failed to delete gist")?
            .error_for_status()
            .context("GitHub API error deleting gist")?;
        Ok(())
    }
}
