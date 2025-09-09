use anyhow::{Context, Result};
use std::process::Command;

pub struct GitHubClient;

impl GitHubClient {
    pub fn new(_token: Option<&str>) -> Result<Self> {
        // Check if gh CLI is available
        let output = Command::new("gh")
            .arg("auth")
            .arg("status")
            .output()
            .context("Failed to run 'gh' command. Please install GitHub CLI (gh) and authenticate with 'gh auth login'")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("GitHub CLI authentication failed: {}", stderr));
        }

        Ok(Self)
    }

    pub async fn create_private_repo(&self, repo_name: &str) -> Result<String> {
        // Create repository using gh CLI
        let output = Command::new("gh")
            .args(&["repo", "create", repo_name, "--private", "--clone=false"])
            .output()
            .context("Failed to create GitHub repository with gh CLI")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Failed to create GitHub repository: {}", stderr));
        }

        // Get the clone URL (use sshUrl for git operations)
        let output = Command::new("gh")
            .args(&["repo", "view", repo_name, "--json", "sshUrl", "-q", ".sshUrl"])
            .output()
            .context("Failed to get repository clone URL")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Failed to get repository clone URL: {}", stderr));
        }

        let clone_url = String::from_utf8(output.stdout)
            .context("Invalid UTF-8 in clone URL response")?
            .trim()
            .to_string();

        Ok(clone_url)
    }

    pub async fn repo_exists(&self, owner: &str, repo_name: &str) -> Result<bool> {
        let repo_full_name = format!("{}/{}", owner, repo_name);
        let output = Command::new("gh")
            .args(&["repo", "view", &repo_full_name])
            .output()
            .context("Failed to check repository existence with gh CLI")?;

        Ok(output.status.success())
    }

    pub async fn get_authenticated_user(&self) -> Result<String> {
        let output = Command::new("gh")
            .args(&["api", "user", "--jq", ".login"])
            .output()
            .context("Failed to get authenticated user with gh CLI")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Failed to get authenticated user: {}", stderr));
        }

        let username = String::from_utf8(output.stdout)
            .context("Invalid UTF-8 in username response")?
            .trim()
            .to_string();

        Ok(username)
    }
}