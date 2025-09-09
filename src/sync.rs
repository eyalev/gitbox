use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct GitboxMetadata {
    pub files: HashMap<String, FileInfo>,
    pub repo_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub id: String,
    pub original_path: PathBuf,
    pub synced_path: PathBuf,
    pub is_directory: bool,
}

impl GitboxMetadata {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            repo_name: None,
        }
    }

    pub fn load_from_dir(dir: &Path) -> Result<Self> {
        let gitbox_file = dir.join(".gitbox");
        if gitbox_file.exists() {
            let content = fs::read_to_string(&gitbox_file)
                .with_context(|| format!("Failed to read .gitbox file: {:?}", gitbox_file))?;
            
            // Try JSON first, then fall back to TOML for backward compatibility
            let metadata: GitboxMetadata = match serde_json::from_str(&content) {
                Ok(metadata) => metadata,
                Err(_) => {
                    // Try parsing as TOML (legacy format)
                    toml::from_str(&content)
                        .with_context(|| "Failed to parse .gitbox file as JSON or TOML")?
                }
            };
            Ok(metadata)
        } else {
            Ok(Self::new())
        }
    }

    pub fn save_to_dir(&self, dir: &Path) -> Result<()> {
        let gitbox_file = dir.join(".gitbox");
        let content = serde_json::to_string_pretty(self)
            .with_context(|| "Failed to serialize metadata")?;
        fs::write(&gitbox_file, content)
            .with_context(|| format!("Failed to write .gitbox file: {:?}", gitbox_file))?;
        Ok(())
    }

    pub fn add_file(&mut self, original_path: &Path, synced_path: &Path, is_directory: bool) -> String {
        let id = Uuid::new_v4().to_string();
        let file_info = FileInfo {
            id: id.clone(),
            original_path: original_path.to_path_buf(),
            synced_path: synced_path.to_path_buf(),
            is_directory,
        };
        
        let key = original_path.to_string_lossy().to_string();
        self.files.insert(key, file_info);
        id
    }

    pub fn remove_file(&mut self, original_path: &Path) -> Option<FileInfo> {
        let key = original_path.to_string_lossy().to_string();
        self.files.remove(&key)
    }

    pub fn get_file(&self, original_path: &Path) -> Option<&FileInfo> {
        let key = original_path.to_string_lossy().to_string();
        self.files.get(&key)
    }
}

pub fn create_link(original: &Path, link: &Path) -> Result<()> {
    if link.exists() {
        fs::remove_file(link)
            .with_context(|| format!("Failed to remove existing link: {:?}", link))?;
    }

    if let Some(parent) = link.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create parent directory: {:?}", parent))?;
        }
    }

    // For directories, we must use symlinks (hard links don't work for directories)
    if original.is_dir() {
        return create_symlink(original, link);
    }

    // For files, try hard link first, fall back to symlink if it fails
    match fs::hard_link(original, link) {
        Ok(()) => {
            println!("Created hard link: {} -> {}", 
                original.display(), 
                link.display()
            );
            Ok(())
        }
        Err(e) => {
            // Hard link failed (likely different filesystems), fall back to symlink
            eprintln!("Hard link failed ({}), falling back to symlink", e);
            create_symlink(original, link)
        }
    }
}

fn create_symlink(original: &Path, link: &Path) -> Result<()> {
    #[cfg(unix)]
    std::os::unix::fs::symlink(original, link)
        .with_context(|| format!("Failed to create symlink from {:?} to {:?}", original, link))?;

    #[cfg(windows)]
    {
        if original.is_dir() {
            std::os::windows::fs::symlink_dir(original, link)
                .with_context(|| format!("Failed to create directory symlink from {:?} to {:?}", original, link))?;
        } else {
            std::os::windows::fs::symlink_file(original, link)
                .with_context(|| format!("Failed to create file symlink from {:?} to {:?}", original, link))?;
        }
    }

    println!("Created symlink: {} -> {}", 
        original.display(), 
        link.display()
    );
    Ok(())
}