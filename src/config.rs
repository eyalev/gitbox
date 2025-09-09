use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use dirs::home_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub github_token: Option<String>,
    pub default_branch: String,
    pub repos_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        let gitbox_dir = Self::gitbox_dir();
        Self {
            github_token: None,
            default_branch: "main".to_string(),
            repos_dir: gitbox_dir.join("repos"),
        }
    }
}

impl Config {
    pub fn gitbox_dir() -> PathBuf {
        home_dir()
            .expect("Could not find home directory")
            .join(".gitbox")
    }

    pub fn config_path() -> PathBuf {
        Self::gitbox_dir().join("config.toml")
    }

    pub fn load_or_create() -> Result<Self> {
        let config_path = Self::config_path();
        let gitbox_dir = Self::gitbox_dir();

        // Create .gitbox directory if it doesn't exist
        if !gitbox_dir.exists() {
            fs::create_dir_all(&gitbox_dir)
                .with_context(|| format!("Failed to create directory: {:?}", gitbox_dir))?;
        }

        // Load existing config or create default
        if config_path.exists() {
            let content = fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read config file: {:?}", config_path))?;
            let config: Config = toml::from_str(&content)
                .with_context(|| "Failed to parse config file")?;
            
            // Ensure repos directory exists
            if !config.repos_dir.exists() {
                fs::create_dir_all(&config.repos_dir)
                    .with_context(|| format!("Failed to create repos directory: {:?}", config.repos_dir))?;
            }
            
            Ok(config)
        } else {
            let config = Config::default();
            config.save()?;
            
            // Create repos directory
            fs::create_dir_all(&config.repos_dir)
                .with_context(|| format!("Failed to create repos directory: {:?}", config.repos_dir))?;
            
            Ok(config)
        }
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path();
        let content = toml::to_string_pretty(self)
            .with_context(|| "Failed to serialize config")?;
        fs::write(&config_path, content)
            .with_context(|| format!("Failed to write config file: {:?}", config_path))?;
        Ok(())
    }

    pub fn set_github_token(&mut self, token: String) -> Result<()> {
        self.github_token = Some(token);
        self.save()
    }

    pub fn get_repo_path(&self, repo_name: &str) -> PathBuf {
        self.repos_dir.join(repo_name)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RepoInfo {
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub file_count: usize,
    pub remote_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppInfo {
    pub version: String,
    pub created_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub repositories: HashMap<String, RepoInfo>,
    pub total_repos: usize,
    pub total_files: usize,
}

impl Default for AppInfo {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            created_at: now,
            last_updated: now,
            repositories: HashMap::new(),
            total_repos: 0,
            total_files: 0,
        }
    }
}

impl AppInfo {
    pub fn info_path() -> PathBuf {
        Config::gitbox_dir().join("info.json")
    }

    pub fn load_or_create() -> Result<Self> {
        let info_path = Self::info_path();
        
        if info_path.exists() {
            let content = fs::read_to_string(&info_path)
                .with_context(|| format!("Failed to read info file: {:?}", info_path))?;
            let info: AppInfo = serde_json::from_str(&content)
                .with_context(|| "Failed to parse info file")?;
            Ok(info)
        } else {
            let info = AppInfo::default();
            info.save()?;
            Ok(info)
        }
    }

    pub fn save(&self) -> Result<()> {
        let info_path = Self::info_path();
        let content = serde_json::to_string_pretty(self)
            .with_context(|| "Failed to serialize app info")?;
        fs::write(&info_path, content)
            .with_context(|| format!("Failed to write info file: {:?}", info_path))?;
        Ok(())
    }

    pub fn add_repository(&mut self, name: &str, remote_url: Option<String>) -> Result<()> {
        let now = Utc::now();
        let repo_info = RepoInfo {
            name: name.to_string(),
            created_at: now,
            last_updated: now,
            file_count: 0,
            remote_url,
        };
        
        self.repositories.insert(name.to_string(), repo_info);
        self.total_repos = self.repositories.len();
        self.last_updated = now;
        self.save()
    }

    pub fn remove_repository(&mut self, name: &str) -> Result<()> {
        if let Some(repo_info) = self.repositories.remove(name) {
            self.total_files = self.total_files.saturating_sub(repo_info.file_count);
        }
        self.total_repos = self.repositories.len();
        self.last_updated = Utc::now();
        self.save()
    }

    pub fn update_repository(&mut self, name: &str, file_count: usize) -> Result<()> {
        if let Some(repo_info) = self.repositories.get_mut(name) {
            let old_count = repo_info.file_count;
            repo_info.file_count = file_count;
            repo_info.last_updated = Utc::now();
            
            // Update total files count
            self.total_files = self.total_files.saturating_sub(old_count) + file_count;
            self.last_updated = Utc::now();
            self.save()?;
        }
        Ok(())
    }

    pub fn refresh_from_disk(&mut self, config: &Config) -> Result<()> {
        // Scan the repos directory and update info
        if !config.repos_dir.exists() {
            return Ok(());
        }

        let mut found_repos = HashMap::new();
        let mut total_files = 0;

        for entry in fs::read_dir(&config.repos_dir)
            .with_context(|| format!("Failed to read repos directory: {:?}", config.repos_dir))? {
            let entry = entry.context("Failed to read directory entry")?;
            let path = entry.path();
            
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    // Count files in this repo
                    let files_dir = path.join("files");
                    let file_count = if files_dir.exists() {
                        fs::read_dir(&files_dir)
                            .map(|entries| entries.count())
                            .unwrap_or(0)
                    } else {
                        0
                    };

                    // Get remote URL if available
                    let remote_url = std::process::Command::new("git")
                        .args(&["remote", "get-url", "origin"])
                        .current_dir(&path)
                        .output()
                        .ok()
                        .filter(|output| output.status.success())
                        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());

                    // Use existing repo info or create new one
                    let repo_info = if let Some(existing) = self.repositories.get(name) {
                        RepoInfo {
                            name: name.to_string(),
                            created_at: existing.created_at,
                            last_updated: Utc::now(),
                            file_count,
                            remote_url: remote_url.or_else(|| existing.remote_url.clone()),
                        }
                    } else {
                        RepoInfo {
                            name: name.to_string(),
                            created_at: Utc::now(),
                            last_updated: Utc::now(),
                            file_count,
                            remote_url,
                        }
                    };

                    total_files += file_count;
                    found_repos.insert(name.to_string(), repo_info);
                }
            }
        }

        self.repositories = found_repos;
        self.total_repos = self.repositories.len();
        self.total_files = total_files;
        self.last_updated = Utc::now();
        self.save()
    }
}