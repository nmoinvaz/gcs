use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::config::ProjectConfig;
use crate::gist::GistClient;
use crate::manifest::{current_platform, Manifest, ManifestFile};

/// Tolerance for comparing local mtime against the manifest timestamp.
/// Accounts for coarser filesystem mtime resolution (e.g. FAT's two-second
/// granularity) and minor clock drift between machines.
const MTIME_TOLERANCE_SECS: i64 = 1;

/// Scan files for secrets and bail if any are found.
fn check_secrets(config: &ProjectConfig, paths: &[String]) -> Result<()> {
    let patterns = secretscan::patterns::get_all_patterns();
    let mut all_findings = Vec::new();

    for path in paths {
        let full = config.root.join(path);
        let content = match fs::read_to_string(&full) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for (line_num, line) in content.lines().enumerate() {
            for (name, regex) in patterns.iter() {
                if regex.is_match(line) {
                    all_findings.push(format!(
                        "  {}:{} [{}] {}",
                        path,
                        line_num + 1,
                        name,
                        line.trim()
                    ));
                }
            }
        }
    }

    if !all_findings.is_empty() {
        eprintln!("Potential secrets found:");
        for f in &all_findings {
            eprintln!("{f}");
        }
        anyhow::bail!(
            "Aborting — {} potential secret(s) detected. Review and remove them before pushing.",
            all_findings.len()
        );
    }
    Ok(())
}

/// Read the manifest from an existing gist.
fn read_manifest(
    client: &GistClient,
    gist_id: &str,
    config: &ProjectConfig,
) -> Result<Option<Manifest>> {
    let gist = client.get_gist(gist_id)?;
    let manifest_name = config.manifest_name();
    match client.get_file_content(&gist, &manifest_name) {
        Some(yaml) => Ok(Manifest::from_yaml(yaml)),
        None => Ok(None),
    }
}

/// Read a local file's modification time as a UTC timestamp.
fn local_mtime(config: &ProjectConfig, path: &str) -> Option<DateTime<Utc>> {
    let full = config.root.join(path);
    let meta = fs::metadata(&full).ok()?;
    let mtime = meta.modified().ok()?;
    Some(DateTime::<Utc>::from(mtime))
}

/// Set a local file's modification time to the given UTC timestamp.
fn set_local_mtime(path: &Path, time: DateTime<Utc>) -> Result<()> {
    let system_time: std::time::SystemTime = time.into();
    let file = fs::File::options()
        .write(true)
        .open(path)
        .with_context(|| format!("Failed to open {} to set mtime", path.display()))?;
    file.set_modified(system_time)
        .with_context(|| format!("Failed to set mtime on {}", path.display()))?;
    Ok(())
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
    // No gist yet: create one from the files supplied on the command line.
    let Some(id) = gist_id else {
        return create_new_gist(client, config, arg_files);
    };

    let mut manifest = read_manifest(client, id, config)?.context("No manifest found in gist")?;

    // Optionally narrow the sync to a specific subset. Files listed on the
    // command line must already be tracked — adding new files is `gcs add`.
    let focus: Option<HashSet<String>> = if arg_files.is_empty() {
        None
    } else {
        let tracked: HashSet<String> = manifest.files.iter().map(|f| f.path.clone()).collect();
        let mut set = HashSet::new();
        for f in arg_files {
            let norm = config.relative_path(f);
            if !tracked.contains(&norm) {
                anyhow::bail!("{norm} is not tracked. Use `gcs add` to track new files.");
            }
            set.insert(norm);
        }
        Some(set)
    };

    let current = current_platform();

    // Classify each relevant manifest entry as push, pull, or in-sync.
    enum Action {
        Push(DateTime<Utc>),
        Pull,
        InSync,
    }

    let mut plan: Vec<(usize, Action)> = Vec::new();
    for (idx, entry) in manifest.files.iter().enumerate() {
        let platform_match = entry.platform.is_none() || entry.platform.as_deref() == Some(current);
        if !platform_match {
            continue;
        }
        if let Some(ref focus) = focus {
            if !focus.contains(&entry.path) {
                continue;
            }
        }

        let action = match local_mtime(config, &entry.path) {
            None => Action::Pull,
            Some(local) => {
                let delta = (local - entry.updated_at).num_seconds();
                if delta > MTIME_TOLERANCE_SECS {
                    Action::Push(local)
                } else if delta < -MTIME_TOLERANCE_SECS {
                    Action::Pull
                } else {
                    Action::InSync
                }
            }
        };
        plan.push((idx, action));
    }

    let mut push_indices: Vec<(usize, DateTime<Utc>)> = Vec::new();
    let mut pull_indices: Vec<usize> = Vec::new();
    let mut in_sync_count = 0usize;
    for (idx, action) in plan {
        match action {
            Action::Push(ts) => push_indices.push((idx, ts)),
            Action::Pull => pull_indices.push(idx),
            Action::InSync => in_sync_count += 1,
        }
    }

    // Pull first so a failed push doesn't leave local out of date.
    if !pull_indices.is_empty() {
        let gist = client.get_gist(id)?;
        for idx in &pull_indices {
            let entry = &manifest.files[*idx];
            let Some(content) = client.get_file_content(&gist, &entry.gist) else {
                eprintln!("  warning: {} not found in gist", entry.gist);
                continue;
            };
            let target = config.root.join(&entry.path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory for {}", entry.path))?;
            }
            fs::write(&target, format!("{content}\n"))
                .with_context(|| format!("Failed to write {}", entry.path))?;
            set_local_mtime(&target, entry.updated_at)?;
            println!("  pulled {} -> {}", entry.gist, entry.path);
        }
    }

    if !push_indices.is_empty() {
        let push_paths: Vec<String> = push_indices
            .iter()
            .map(|(idx, _)| manifest.files[*idx].path.clone())
            .collect();
        check_secrets(config, &push_paths)?;

        for (idx, ts) in &push_indices {
            manifest.files[*idx].updated_at = *ts;
        }

        let mut updates: HashMap<String, Option<String>> = HashMap::new();
        updates.insert(config.manifest_name(), Some(manifest.to_yaml()));
        for (idx, _) in &push_indices {
            let entry = &manifest.files[*idx];
            let full = config.root.join(&entry.path);
            let content = fs::read_to_string(&full)
                .with_context(|| format!("Failed to read {}", entry.path))?;
            updates.insert(entry.gist.clone(), Some(content));
            println!("  pushed {}", entry.path);
        }
        client.update_files(id, &updates)?;
    }

    println!(
        "Summary: {} pushed, {} pulled, {} already in sync",
        push_indices.len(),
        pull_indices.len(),
        in_sync_count
    );
    println!("Gist: https://gist.github.com/{id}");

    Ok(())
}

/// Create a new gist from the files specified on the command line.
fn create_new_gist(
    client: &GistClient,
    config: &ProjectConfig,
    arg_files: &[String],
) -> Result<()> {
    if arg_files.is_empty() {
        anyhow::bail!("No files specified and no manifest found.");
    }

    let normalized: Vec<String> = arg_files.iter().map(|p| config.relative_path(p)).collect();
    let entries: Vec<ManifestFile> = normalized
        .iter()
        .map(|p| {
            let ts = local_mtime(config, p).unwrap_or_else(Utc::now);
            Manifest::entry(config, p, None, ts)
        })
        .collect();

    let manifest = Manifest::new(config, &entries);
    let local_files: Vec<String> = normalized
        .iter()
        .filter(|p| config.root.join(p).is_file())
        .cloned()
        .collect();

    if local_files.is_empty() {
        println!("No tracked config files found and no remote gist exists.");
        return Ok(());
    }

    check_secrets(config, &local_files)?;
    let files = build_file_map(config, &local_files, &manifest);
    println!("Creating gist: {}", config.gist_description());
    let id = client.create_gist(&config.gist_description(), config.public, &files)?;
    println!(
        "Pushed {} file(s): {}",
        local_files.len(),
        local_files.join(" ")
    );
    println!("Gist: https://gist.github.com/{id}");
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
        let normalized = config.relative_path(f);
        if !existing_paths.contains(&normalized) {
            let ts = local_mtime(config, &normalized).unwrap_or_else(Utc::now);
            entries.push(Manifest::entry(config, &normalized, platform, ts));
            added.push(normalized);
        }
    }

    let manifest = Manifest::new(config, &entries);
    check_secrets(config, &added)?;

    if let Some(id) = gist_id {
        let mut updates: HashMap<String, Option<String>> = HashMap::new();
        updates.insert(config.manifest_name(), Some(manifest.to_yaml()));
        for f in &added {
            let full = config.root.join(f);
            if full.is_file() {
                let content =
                    fs::read_to_string(&full).with_context(|| format!("Failed to read {f}"))?;
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
        let local_files: Vec<String> = added
            .iter()
            .filter(|p| config.root.join(p).is_file())
            .cloned()
            .collect();
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

    let manifest = read_manifest(client, id, config)?.context("No manifest found in gist")?;

    let remove_set: HashSet<String> = files.iter().map(|f| config.relative_path(f)).collect();

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
    let manifest = read_manifest(client, id, config)?.context("No manifest found in gist")?;
    let paths = manifest.paths();
    remove_stale_files(client, id, config, &paths)?;
    println!("Gist: https://gist.github.com/{id}");
    Ok(())
}

// -------------------------------------------------------------------------
//  delete
// -------------------------------------------------------------------------

pub fn do_delete(client: &GistClient, gist_id: Option<&str>) -> Result<()> {
    let id = gist_id.context("No gist found")?;
    client.delete_gist(id)?;
    println!("Deleted gist: https://gist.github.com/{id}");
    Ok(())
}

// -------------------------------------------------------------------------
//  open
// -------------------------------------------------------------------------

pub fn do_open(gist_id: Option<&str>) -> Result<()> {
    let id = gist_id.context("No gist found")?;
    let url = format!("https://gist.github.com/{id}");
    println!("Opening {url}");

    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };

    std::process::Command::new(program)
        .arg(&url)
        .status()
        .with_context(|| format!("Failed to launch {program}"))?;

    Ok(())
}
