use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::fs;

use crate::config::ProjectConfig;
use crate::gist::GistClient;
use crate::manifest::{current_platform, Manifest, ManifestFile};

/// Read the manifest from an existing gist.
fn read_manifest(client: &GistClient, gist_id: &str, config: &ProjectConfig) -> Result<Option<Manifest>> {
    let gist = client.get_gist(gist_id)?;
    let manifest_name = config.manifest_name();
    match client.get_file_content(&gist, &manifest_name) {
        Some(yaml) => Ok(Manifest::from_yaml(yaml)),
        None => Ok(None),
    }
}

/// Get the newest mtime across a set of local files, returning (existing_files, max_epoch).
fn local_file_state(config: &ProjectConfig, paths: &[String]) -> (Vec<String>, Option<DateTime<Utc>>) {
    let mut existing = Vec::new();
    let mut max_epoch: Option<DateTime<Utc>> = None;

    for path in paths {
        let full = config.root.join(path);
        if full.is_file() {
            existing.push(path.clone());
            if let Ok(meta) = fs::metadata(&full) {
                if let Ok(mtime) = meta.modified() {
                    let dt: DateTime<Utc> = mtime.into();
                    max_epoch = Some(match max_epoch {
                        Some(prev) if prev > dt => prev,
                        _ => dt,
                    });
                }
            }
        }
    }

    (existing, max_epoch)
}

/// Build the file map for creating or pushing to a gist.
fn build_file_map(
    config: &ProjectConfig,
    paths: &[String],
    manifest: &Manifest,
) -> HashMap<String, String> {
    let mut files = HashMap::new();
    files.insert(config.manifest_name(), manifest.to_yaml());
    for path in paths {
        let full = config.root.join(path);
        if let Ok(content) = fs::read_to_string(&full) {
            files.insert(config.gist_filename(path), content);
        }
    }
    files
}

/// Remove gist files that are not in the expected set.
fn remove_stale_files(
    client: &GistClient,
    gist_id: &str,
    config: &ProjectConfig,
    paths: &[String],
) -> Result<()> {
    let gist = client.get_gist(gist_id)?;
    let remote_files = client.get_file_names(&gist);

    let mut expected: HashSet<String> = HashSet::new();
    expected.insert(config.manifest_name());
    for path in paths {
        expected.insert(config.gist_filename(path));
    }

    let mut deletions: HashMap<String, Option<String>> = HashMap::new();
    for name in &remote_files {
        if !expected.contains(name) {
            deletions.insert(name.clone(), None);
            println!("  removed stale file: {name}");
        }
    }

    if !deletions.is_empty() {
        client.update_files(gist_id, &deletions)?;
    }
    Ok(())
}

// -------------------------------------------------------------------------
//  sync
// -------------------------------------------------------------------------

pub fn do_sync(
    client: &GistClient,
    config: &ProjectConfig,
    arg_files: &[String],
    gist_id: Option<&str>,
) -> Result<()> {
    // Resolve file list and manifest entries.
    let (manifest_files, all_paths, pull_paths) = if !arg_files.is_empty() {
        let entries: Vec<ManifestFile> = arg_files
            .iter()
            .map(|p| Manifest::entry(config, p, None))
            .collect();
        let paths = arg_files.to_vec();
        (entries, paths.clone(), paths)
    } else if let Some(id) = gist_id {
        let manifest = read_manifest(client, id, config)?
            .context("No manifest found in gist")?;
        let all = manifest.paths();
        let pull = manifest.paths_for_current_platform();
        println!("Read {} file(s) from manifest ({} for this platform)", all.len(), pull.len());
        (manifest.files.clone(), all, pull)
    } else {
        anyhow::bail!("No files specified and no manifest found.");
    };

    // For push direction, use all paths. For pull, use platform-filtered paths.
    let (local_files, local_max) = local_file_state(config, &pull_paths);

    if local_files.is_empty() && gist_id.is_none() {
        println!("No tracked config files found and no remote gist exists.");
        return Ok(());
    }

    enum Direction { Create, Push, Pull, InSync }

    let direction = if gist_id.is_none() {
        Direction::Create
    } else if local_files.is_empty() {
        Direction::Pull
    } else {
        let gist = client.get_gist(gist_id.unwrap())?;
        let remote_time = client.get_updated_at(&gist);
        match (local_max, remote_time) {
            (Some(local), Some(remote)) if local > remote => Direction::Push,
            (Some(local), Some(remote)) if remote > local => Direction::Pull,
            (Some(_), Some(_)) => Direction::InSync,
            (Some(_), None) => Direction::Push,
            (None, Some(_)) => Direction::Pull,
            _ => Direction::InSync,
        }
    };

    match direction {
        Direction::Create => {
            let manifest = Manifest::new(config, &manifest_files);
            let files = build_file_map(config, &local_files, &manifest);
            println!("Creating gist: {}", config.gist_description());
            let id = client.create_gist(&config.gist_description(), config.public, &files)?;
            println!("Pushed {} file(s): {}", local_files.len(), local_files.join(" "));
            println!("Gist: https://gist.github.com/{id}");
        }
        Direction::Push => {
            let id = gist_id.unwrap();
            println!("Local is newer — pushing to gist {id}");
            let manifest = Manifest::new(config, &manifest_files);
            // Push only files for this platform (or universal).
            let file_map = build_file_map(config, &local_files, &manifest);
            let updates: HashMap<String, Option<String>> = file_map
                .into_iter()
                .map(|(k, v)| (k, Some(v)))
                .collect();
            client.update_files(id, &updates)?;
            for f in &local_files {
                println!("  pushed {f}");
            }
            remove_stale_files(client, id, config, &all_paths)?;
            println!("Pushed {} file(s)", local_files.len());
            println!("Gist: https://gist.github.com/{id}");
        }
        Direction::Pull => {
            let id = gist_id.unwrap();
            println!("Remote is newer — pulling from gist {id}");
            let gist = client.get_gist(id)?;
            let mut pulled = 0;
            // Only pull files for this platform.
            for path in &pull_paths {
                let gist_name = config.gist_filename(path);
                if let Some(content) = client.get_file_content(&gist, &gist_name) {
                    let target = config.root.join(path);
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)
                            .with_context(|| format!("Failed to create directory for {path}"))?;
                    }
                    fs::write(&target, format!("{content}\n"))
                        .with_context(|| format!("Failed to write {path}"))?;
                    println!("  pulled {gist_name} -> {path}");
                    pulled += 1;
                }
            }
            println!("Pulled {pulled} file(s)");
            println!("Gist: https://gist.github.com/{id}");
        }
        Direction::InSync => {
            let id = gist_id.unwrap();
            println!("Already in sync.");
            println!("Gist: https://gist.github.com/{id}");
        }
    }

    Ok(())
}

// -------------------------------------------------------------------------
//  add
// -------------------------------------------------------------------------

pub fn do_add(
    client: &GistClient,
    config: &ProjectConfig,
    files: &[String],
    platform_specific: bool,
    gist_id: Option<&str>,
) -> Result<()> {
    if files.is_empty() {
        anyhow::bail!("Usage: gcs add FILE...");
    }

    let platform = if platform_specific {
        Some(current_platform())
    } else {
        None
    };

    // Start with existing manifest entries, then merge new ones.
    let mut entries: Vec<ManifestFile> = if let Some(id) = gist_id {
        read_manifest(client, id, config)?
            .map(|m| m.files)
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let existing_paths: HashSet<String> = entries.iter().map(|e| e.path.clone()).collect();
    let mut added = Vec::new();
    for f in files {
        let normalized = f.strip_prefix("./").unwrap_or(f).to_string();
        if !existing_paths.contains(&normalized) {
            entries.push(Manifest::entry(config, &normalized, platform));
            added.push(normalized);
        }
    }

    let manifest = Manifest::new(config, &entries);

    if let Some(id) = gist_id {
        let mut updates: HashMap<String, Option<String>> = HashMap::new();
        updates.insert(config.manifest_name(), Some(manifest.to_yaml()));
        for f in &added {
            let full = config.root.join(f);
            if full.is_file() {
                let content = fs::read_to_string(&full)
                    .with_context(|| format!("Failed to read {f}"))?;
                updates.insert(config.gist_filename(f), Some(content));
                let suffix = platform.map(|p| format!(" ({p})")).unwrap_or_default();
                println!("  added {f}{suffix}");
            } else {
                println!("  skipped {f} (not found locally)");
            }
        }
        client.update_files(id, &updates)?;
        println!("Gist: https://gist.github.com/{id}");
    } else {
        let all_paths = manifest.paths();
        let (local_files, _) = local_file_state(config, &all_paths);
        let file_map = build_file_map(config, &local_files, &manifest);
        println!("Creating gist: {}", config.gist_description());
        let id = client.create_gist(&config.gist_description(), config.public, &file_map)?;
        for f in &added {
            let suffix = platform.map(|p| format!(" ({p})")).unwrap_or_default();
            println!("  added {f}{suffix}");
        }
        println!("Gist: https://gist.github.com/{id}");
    }

    Ok(())
}

// -------------------------------------------------------------------------
//  remove
// -------------------------------------------------------------------------

pub fn do_remove(
    client: &GistClient,
    config: &ProjectConfig,
    files: &[String],
    gist_id: Option<&str>,
) -> Result<()> {
    if files.is_empty() {
        anyhow::bail!("Usage: gcs remove FILE...");
    }

    let id = gist_id.context("No gist found")?;

    let manifest = read_manifest(client, id, config)?
        .context("No manifest found in gist")?;

    let remove_set: HashSet<String> = files
        .iter()
        .map(|f| f.strip_prefix("./").unwrap_or(f).to_string())
        .collect();

    let remaining: Vec<ManifestFile> = manifest
        .files
        .into_iter()
        .filter(|e| !remove_set.contains(&e.path))
        .collect();

    let mut updates: HashMap<String, Option<String>> = HashMap::new();
    for f in &remove_set {
        updates.insert(config.gist_filename(f), None);
        println!("  removed {f}");
    }
    let new_manifest = Manifest::new(config, &remaining);
    updates.insert(config.manifest_name(), Some(new_manifest.to_yaml()));

    client.update_files(id, &updates)?;
    println!("Gist: https://gist.github.com/{id}");

    Ok(())
}

// -------------------------------------------------------------------------
//  cleanup
// -------------------------------------------------------------------------

pub fn do_cleanup(
    client: &GistClient,
    config: &ProjectConfig,
    gist_id: Option<&str>,
) -> Result<()> {
    let id = gist_id.context("No gist found")?;
    let manifest = read_manifest(client, id, config)?
        .context("No manifest found in gist")?;
    let paths = manifest.paths();
    remove_stale_files(client, id, config, &paths)?;
    println!("Gist: https://gist.github.com/{id}");
    Ok(())
}

// -------------------------------------------------------------------------
//  delete
// -------------------------------------------------------------------------

pub fn do_delete(
    client: &GistClient,
    gist_id: Option<&str>,
) -> Result<()> {
    let id = gist_id.context("No gist found")?;
    client.delete_gist(id)?;
    println!("Deleted gist: https://gist.github.com/{id}");
    Ok(())
}
