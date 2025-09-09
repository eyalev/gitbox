use anyhow::{Context, Result};
use git2::{Repository, Signature, IndexAddOption};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{Config, AppInfo};
use crate::github::GitHubClient;
use crate::sync::{GitboxMetadata, create_link};

pub struct RepoManager {
    config: Config,
    app_info: AppInfo,
}

impl RepoManager {
    pub fn new(config: &Config) -> Result<Self> {
        // Validate that repos directory exists
        if !config.repos_dir.exists() {
            fs::create_dir_all(&config.repos_dir)
                .with_context(|| format!("Failed to create repos directory: {:?}", config.repos_dir))?;
        }

        // Load or create app info
        let mut app_info = AppInfo::load_or_create()?;
        
        // Refresh app info from disk to ensure it's up to date
        app_info.refresh_from_disk(config)?;

        Ok(Self {
            config: config.clone(),
            app_info,
        })
    }

    pub async fn add_repo(&mut self, repo_name: &str) -> Result<()> {
        // Validate repo name
        if repo_name.trim().is_empty() {
            return Err(anyhow::anyhow!("Repository name cannot be empty"));
        }
        
        if repo_name.contains('/') || repo_name.contains('\\') {
            return Err(anyhow::anyhow!("Repository name cannot contain path separators"));
        }

        let repo_path = self.config.get_repo_path(repo_name);
        
        if repo_path.exists() {
            return Err(anyhow::anyhow!("Repository '{}' already exists", repo_name));
        }

        // Create repository directory
        fs::create_dir_all(&repo_path)
            .with_context(|| format!("Failed to create repository directory: {:?}", repo_path))?;

        // Initialize git repository
        let git_repo = Repository::init(&repo_path)
            .with_context(|| format!("Failed to initialize git repository: {:?}", repo_path))?;

        // Create initial .gitbox metadata file
        let mut metadata = GitboxMetadata::new();
        metadata.repo_name = Some(repo_name.to_string());
        metadata.save_to_dir(&repo_path)?;

        // Create initial commit
        let signature = Signature::now("gitbox", "gitbox@local")
            .context("Failed to create git signature")?;
        
        let mut index = git_repo.index()
            .context("Failed to get git index")?;
        index.add_path(Path::new(".gitbox"))
            .context("Failed to add .gitbox to index")?;
        index.write()
            .context("Failed to write git index")?;

        let tree_id = index.write_tree()
            .context("Failed to write git tree")?;
        let tree = git_repo.find_tree(tree_id)
            .context("Failed to find git tree")?;

        // For the first commit, we need to update the HEAD reference manually
        let commit_id = git_repo.commit(
            None, // Don't update any reference yet
            &signature,
            &signature,
            "Initial commit",
            &tree,
            &[],
        ).context("Failed to create initial commit")?;

        // Set HEAD to point to the new commit
        git_repo.reference(&format!("refs/heads/{}", self.config.default_branch), commit_id, false, "Initial commit")
            .context("Failed to create branch reference")?;
        
        // Set HEAD to point to the branch
        git_repo.set_head(&format!("refs/heads/{}", self.config.default_branch))
            .context("Failed to set HEAD")?;

        // Create or get existing GitHub repository
        let github_client = GitHubClient::new(self.config.github_token.as_deref())?;
        let clone_url = match github_client.create_private_repo(repo_name).await {
            Ok(url) => {
                println!("Created new GitHub repository");
                url
            }
            Err(e) => {
                // Check if the error is because the repository already exists
                let error_msg = format!("{}", e);
                if error_msg.contains("Name already exists") || error_msg.contains("already exists") {
                    println!("GitHub repository already exists, syncing with existing repository...");
                    
                    // Get the authenticated user to construct the clone URL
                    let username = github_client.get_authenticated_user().await?;
                    format!("git@github.com:{}/{}.git", username, repo_name)
                } else {
                    return Err(e);
                }
            }
        };

        // Add remote and sync with GitHub
        let _remote = git_repo.remote("origin", &clone_url)
            .context("Failed to add remote origin")?;

        // Try to pull first in case the remote repository has content
        let pull_output = std::process::Command::new("git")
            .args(&["pull", "origin", &self.config.default_branch, "--allow-unrelated-histories"])
            .current_dir(&repo_path)
            .output()
            .context("Failed to execute git pull")?;

        if pull_output.status.success() {
            println!("Synced with existing remote repository");
        } else {
            // If pull fails, the remote might be empty, so try to push our initial commit
            let push_output = std::process::Command::new("git")
                .args(&["push", "-u", "origin", &self.config.default_branch])
                .current_dir(&repo_path)
                .output()
                .context("Failed to execute git push")?;

            if !push_output.status.success() {
                let stderr = String::from_utf8_lossy(&push_output.stderr);
                return Err(anyhow::anyhow!("Failed to push to GitHub: {}", stderr));
            }
            println!("Pushed initial commit to remote repository");
        }

        // Update app info with new repository
        self.app_info.add_repository(repo_name, Some(clone_url))?;

        Ok(())
    }

    pub fn delete_repo(&mut self, repo_name: &str, force: bool) -> Result<()> {
        // Try to find the repository with fuzzy matching
        let actual_repo_name = self.find_repository(repo_name)?;
        let repo_path = self.config.get_repo_path(&actual_repo_name);
        
        if !repo_path.exists() {
            return Err(anyhow::anyhow!("Repository '{}' does not exist", actual_repo_name));
        }

        // Load metadata to show what will be deleted
        let metadata = GitboxMetadata::load_from_dir(&repo_path)?;
        
        if !force {
            if actual_repo_name != repo_name {
                println!("Found repository '{}' matching '{}'", actual_repo_name, repo_name);
            }
            println!("Repository '{}' will be deleted:", actual_repo_name);
            println!("  Path: {:?}", repo_path);
            
            if !metadata.files.is_empty() {
                println!("  Synced files ({}):", metadata.files.len());
                for (original_path, file_info) in &metadata.files {
                    let file_type = if file_info.is_directory { "dir" } else { "file" };
                    println!("    {} -> {} ({})", 
                        original_path,
                        file_info.synced_path.display(),
                        file_type
                    );
                }
            } else {
                println!("  No synced files");
            }
            
            // Check if GitHub repository exists
            let remote_check = std::process::Command::new("git")
                .args(&["remote", "get-url", "origin"])
                .current_dir(&repo_path)
                .output();
            
            if let Ok(output) = remote_check {
                if output.status.success() {
                    let remote_url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    println!("  GitHub repository: {}", remote_url);
                    println!("\n⚠️  WARNING: This will only delete the LOCAL repository.");
                    println!("   The GitHub repository will remain online.");
                    println!("   To delete it from GitHub, use: gh repo delete <repo-name>");
                }
            }
            
            println!("\nThis action cannot be undone!");
            print!("Are you sure you want to delete this repository? (y/N): ");
            
            use std::io::{self, Write};
            io::stdout().flush()?;
            
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim().to_lowercase();
            
            if input != "y" && input != "yes" {
                println!("Repository deletion cancelled.");
                return Ok(());
            }
        }

        // Remove the repository directory
        std::fs::remove_dir_all(&repo_path)
            .with_context(|| format!("Failed to delete repository directory: {:?}", repo_path))?;

        // Update app info by removing the repository
        self.app_info.remove_repository(&actual_repo_name)?;

        println!("Repository '{}' has been deleted from local storage.", actual_repo_name);
        
        if !metadata.files.is_empty() {
            println!("\nNote: Symbolic links to synced files may now be broken.");
            println!("You may need to manually clean up broken symlinks in:");
            for (original_path, _) in &metadata.files {
                if let Some(parent) = std::path::Path::new(original_path).parent() {
                    println!("  {:?}", parent);
                }
            }
        }
        
        Ok(())
    }

    pub async fn sync_file_with_default(&mut self, file_path: &str, repo_name: &str) -> Result<()> {
        let repo_path = self.config.get_repo_path(repo_name);
        
        // If repository doesn't exist, create it
        if !repo_path.exists() {
            println!("Repository '{}' doesn't exist. Creating it...", repo_name);
            self.add_repo(repo_name).await?;
            println!("Repository '{}' created successfully", repo_name);
        }

        // Now sync the file
        self.sync_file(file_path, repo_name)
    }

    pub async fn sync_from_remote(&mut self, filename: &str, repo_name: &str) -> Result<()> {
        let repo_path = self.config.get_repo_path(repo_name);
        
        // If repository doesn't exist, create it
        if !repo_path.exists() {
            println!("Repository '{}' doesn't exist. Creating it...", repo_name);
            self.add_repo(repo_name).await?;
            println!("Repository '{}' created successfully", repo_name);
        }

        // First, sync the repository to get latest changes from remote
        self.sync_repo(repo_name)?;

        // Check if the file exists in the repository
        let synced_file_path = repo_path.join("files").join(filename);
        if !synced_file_path.exists() {
            return Err(anyhow::anyhow!("File '{}' not found in repository '{}'", filename, repo_name));
        }

        // Get current directory for the destination
        let current_dir = std::env::current_dir()
            .context("Failed to get current directory")?;
        let destination_path = current_dir.join(filename);

        // Check if file already exists locally
        if destination_path.exists() {
            return Err(anyhow::anyhow!("File '{}' already exists in current directory", filename));
        }

        // Create hard link from repository to current directory
        let is_directory = synced_file_path.is_dir();
        create_link(&synced_file_path, &destination_path)?;

        // Update local metadata
        let mut local_metadata = GitboxMetadata::load_from_dir(&current_dir)?;
        local_metadata.add_file(&destination_path, &synced_file_path, is_directory);
        local_metadata.repo_name = Some(repo_name.to_string());
        local_metadata.save_to_dir(&current_dir)?;

        Ok(())
    }

    pub fn sync_file(&mut self, file_path: &str, repo_name: &str) -> Result<()> {
        let repo_path = self.config.get_repo_path(repo_name);
        if !repo_path.exists() {
            return Err(anyhow::anyhow!("Repository '{}' does not exist", repo_name));
        }

        let original_path = PathBuf::from(file_path);
        if !original_path.exists() {
            return Err(anyhow::anyhow!("File or directory does not exist: {}", file_path));
        }

        let original_path = original_path.canonicalize()
            .with_context(|| format!("Failed to canonicalize path: {}", file_path))?;

        // Load metadata from the current directory
        let current_dir = std::env::current_dir()
            .context("Failed to get current directory")?;
        let mut local_metadata = GitboxMetadata::load_from_dir(&current_dir)?;

        // Check if file is already synced
        if local_metadata.get_file(&original_path).is_some() {
            return Err(anyhow::anyhow!("File is already synced to a repository"));
        }

        // Create symlink path in repository
        let file_name = original_path.file_name()
            .context("Failed to get file name")?;
        let synced_path = repo_path.join("files").join(file_name);

        // Create files directory in repo if it doesn't exist
        let files_dir = repo_path.join("files");
        if !files_dir.exists() {
            fs::create_dir_all(&files_dir)
                .with_context(|| format!("Failed to create files directory: {:?}", files_dir))?;
        }

        // Create link (hard link for files, symlink for directories)
        let is_directory = original_path.is_dir();
        create_link(&original_path, &synced_path)?;

        // Update local metadata
        local_metadata.add_file(&original_path, &synced_path, is_directory);
        local_metadata.repo_name = Some(repo_name.to_string());
        local_metadata.save_to_dir(&current_dir)?;

        // Update repository metadata
        let mut repo_metadata = GitboxMetadata::load_from_dir(&repo_path)?;
        repo_metadata.add_file(&original_path, &synced_path, is_directory);
        repo_metadata.save_to_dir(&repo_path)?;

        // Commit changes
        self.commit_repo_changes(&repo_path, &format!("Add file: {}", file_name.to_string_lossy()))?;
        
        // Push changes to remote repository
        self.push_repo_changes(&repo_path)?;

        // Update app info with new file count
        let files_dir = repo_path.join("files");
        let file_count = if files_dir.exists() {
            fs::read_dir(&files_dir)
                .map(|entries| entries.count())
                .unwrap_or(0)
        } else {
            0
        };
        self.app_info.update_repository(repo_name, file_count)?;

        Ok(())
    }

    pub fn list_repos(&self) -> Result<Vec<String>> {
        let repos_dir = &self.config.repos_dir;
        if !repos_dir.exists() {
            return Ok(vec![]);
        }

        let mut repos = vec![];
        for entry in fs::read_dir(repos_dir)
            .with_context(|| format!("Failed to read repos directory: {:?}", repos_dir))? {
            let entry = entry.context("Failed to read directory entry")?;
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name() {
                    repos.push(name.to_string_lossy().to_string());
                }
            }
        }

        repos.sort();
        Ok(repos)
    }

    pub fn list_repo_files(&self, repo_name: &str) -> Result<Vec<String>> {
        // Try to find the repository with fuzzy matching
        let actual_repo_name = self.find_repository(repo_name)?;
        let repo_path = self.config.get_repo_path(&actual_repo_name);
        if !repo_path.exists() {
            return Err(anyhow::anyhow!("Repository '{}' does not exist", actual_repo_name));
        }

        let files_dir = repo_path.join("files");
        if !files_dir.exists() {
            return Ok(vec![]);
        }

        let mut files = vec![];
        for entry in fs::read_dir(&files_dir)
            .with_context(|| format!("Failed to read files directory: {:?}", files_dir))? {
            let entry = entry.context("Failed to read directory entry")?;
            let path = entry.path();
            if let Some(name) = path.file_name() {
                files.push(name.to_string_lossy().to_string());
            }
        }

        files.sort();
        Ok(files)
    }

    fn find_repository(&self, partial_name: &str) -> Result<String> {
        let repos = self.list_repos()?;
        
        // First try exact match
        if repos.iter().any(|r| r == partial_name) {
            return Ok(partial_name.to_string());
        }
        
        // Then try to find repos that contain the partial name
        let matches: Vec<&String> = repos.iter()
            .filter(|repo| repo.contains(partial_name))
            .collect();
            
        match matches.len() {
            0 => {
                // Try to find repos where partial name is contained in repo
                let reverse_matches: Vec<&String> = repos.iter()
                    .filter(|repo| partial_name.contains(repo.as_str()))
                    .collect();
                    
                if reverse_matches.len() == 1 {
                    Ok(reverse_matches[0].clone())
                } else if reverse_matches.is_empty() {
                    Err(anyhow::anyhow!("No repository found matching '{}'. Available repositories: {}", 
                        partial_name, 
                        if repos.is_empty() { "none".to_string() } else { repos.join(", ") }
                    ))
                } else {
                    let matches_str: Vec<String> = reverse_matches.into_iter().cloned().collect();
                    Err(anyhow::anyhow!("Multiple repositories match '{}': {}. Please be more specific.", 
                        partial_name, 
                        matches_str.join(", ")
                    ))
                }
            }
            1 => Ok(matches[0].clone()),
            _ => {
                let matches_str: Vec<String> = matches.into_iter().cloned().collect();
                Err(anyhow::anyhow!("Multiple repositories match '{}': {}. Please be more specific.", 
                    partial_name, 
                    matches_str.join(", ")
                ))
            }
        }
    }

    pub fn get_repo_info(&self, repo_name: &str) -> Result<String> {
        // Try to find the repository with fuzzy matching
        let actual_repo_name = self.find_repository(repo_name)?;
        let repo_path = self.config.get_repo_path(&actual_repo_name);
        if !repo_path.exists() {
            return Err(anyhow::anyhow!("Repository '{}' does not exist", actual_repo_name));
        }

        let mut info = String::new();
        if actual_repo_name != repo_name {
            info.push_str(&format!("Found repository '{}' matching '{}'\n", actual_repo_name, repo_name));
        }
        info.push_str(&format!("Repository: {}\n", actual_repo_name));
        info.push_str(&format!("Path: {:?}\n", repo_path));

        // Get git repository info
        if let Ok(git_repo) = Repository::open(&repo_path) {
            // Get remote URL
            if let Ok(remote) = git_repo.find_remote("origin") {
                if let Some(url) = remote.url() {
                    info.push_str(&format!("Remote URL: {}\n", url));
                }
            }

            // Get current branch
            if let Ok(head) = git_repo.head() {
                if let Some(name) = head.shorthand() {
                    info.push_str(&format!("Branch: {}\n", name));
                }
            }

            // Get latest commit
            if let Ok(head) = git_repo.head() {
                if let Ok(commit) = head.peel_to_commit() {
                    let message = commit.message().unwrap_or("(no message)");
                    let short_id = commit.id().to_string()[..8].to_string();
                    info.push_str(&format!("Latest commit: {} - {}\n", short_id, message.lines().next().unwrap_or("(no message)")));
                }
            }
        }

        // Get file count
        let files_dir = repo_path.join("files");
        if files_dir.exists() {
            let file_count = fs::read_dir(&files_dir)
                .map(|entries| entries.count())
                .unwrap_or(0);
            info.push_str(&format!("Synced files: {}\n", file_count));
        } else {
            info.push_str("Synced files: 0\n");
        }

        // Load metadata
        if let Ok(metadata) = GitboxMetadata::load_from_dir(&repo_path) {
            info.push_str(&format!("Tracked files: {}\n", metadata.files.len()));
            if !metadata.files.is_empty() {
                info.push_str("Files:\n");
                for (original_path, file_info) in &metadata.files {
                    let file_type = if file_info.is_directory { "dir" } else { "file" };
                    info.push_str(&format!("  {} -> {} ({})\n", 
                        original_path,
                        file_info.synced_path.display(),
                        file_type
                    ));
                }
            }
        }

        Ok(info.trim_end().to_string())
    }

    pub fn sync_repo(&self, repo_name: &str) -> Result<()> {
        // Try to find the repository with fuzzy matching
        let actual_repo_name = self.find_repository(repo_name)?;
        let repo_path = self.config.get_repo_path(&actual_repo_name);
        if !repo_path.exists() {
            return Err(anyhow::anyhow!("Repository '{}' does not exist", actual_repo_name));
        }

        if actual_repo_name != repo_name {
            println!("Found repository '{}' matching '{}'", actual_repo_name, repo_name);
        }

        // Check if remote origin exists
        let remote_check = std::process::Command::new("git")
            .args(&["remote", "get-url", "origin"])
            .current_dir(&repo_path)
            .output()
            .context("Failed to check remote origin")?;

        if !remote_check.status.success() {
            return Err(anyhow::anyhow!("Repository '{}' has no remote origin configured. Please run 'gitbox add-repo {}' first or manually configure the remote.", actual_repo_name, actual_repo_name));
        }

        // Check if we're on the default branch, create it if it doesn't exist
        let branch_check = std::process::Command::new("git")
            .args(&["rev-parse", "--verify", &self.config.default_branch])
            .current_dir(&repo_path)
            .output()
            .context("Failed to check current branch")?;

        if !branch_check.status.success() {
            // Create the default branch if it doesn't exist
            let create_branch = std::process::Command::new("git")
                .args(&["checkout", "-b", &self.config.default_branch])
                .current_dir(&repo_path)
                .output()
                .context("Failed to create default branch")?;

            if !create_branch.status.success() {
                let stderr = String::from_utf8_lossy(&create_branch.stderr);
                return Err(anyhow::anyhow!("Failed to create branch '{}': {}", self.config.default_branch, stderr));
            }
            println!("Created branch '{}'", self.config.default_branch);
        }

        // First, try to pull from remote to get latest changes
        let pull_output = std::process::Command::new("git")
            .args(&["pull", "--no-rebase", "--allow-unrelated-histories", "origin", &self.config.default_branch])
            .current_dir(&repo_path)
            .output()
            .context("Failed to execute git pull")?;

        if !pull_output.status.success() {
            let stderr = String::from_utf8_lossy(&pull_output.stderr);
            // If pull fails due to no upstream, set it up
            if stderr.contains("no upstream") || stderr.contains("couldn't find remote ref") {
                println!("Setting up upstream branch...");
            } else {
                eprintln!("Warning: git pull failed: {}", stderr);
            }
        } else {
            println!("Pulled latest changes from GitHub");
        }

        // Check if there are any changes to commit
        let status_output = std::process::Command::new("git")
            .args(&["status", "--porcelain"])
            .current_dir(&repo_path)
            .output()
            .context("Failed to execute git status")?;

        if !status_output.status.success() {
            return Err(anyhow::anyhow!("Failed to check git status"));
        }

        let has_changes = !status_output.stdout.is_empty();

        if has_changes {
            // Add all changes
            let add_output = std::process::Command::new("git")
                .args(&["add", "."])
                .current_dir(&repo_path)
                .output()
                .context("Failed to execute git add")?;

            if !add_output.status.success() {
                let stderr = String::from_utf8_lossy(&add_output.stderr);
                return Err(anyhow::anyhow!("Failed to add changes: {}", stderr));
            }

            // Commit changes
            let commit_output = std::process::Command::new("git")
                .args(&["commit", "-m", "Update synced files"])
                .current_dir(&repo_path)
                .output()
                .context("Failed to execute git commit")?;

            if !commit_output.status.success() {
                let stderr = String::from_utf8_lossy(&commit_output.stderr);
                return Err(anyhow::anyhow!("Failed to commit changes: {}", stderr));
            }

            println!("Committed local changes");
        } else {
            println!("No local changes to commit");
        }

        // Push to remote (with upstream setup if needed)
        let push_output = std::process::Command::new("git")
            .args(&["push", "-u", "origin", &self.config.default_branch])
            .current_dir(&repo_path)
            .output()
            .context("Failed to execute git push")?;

        if !push_output.status.success() {
            let stderr = String::from_utf8_lossy(&push_output.stderr);
            
            // If push was rejected due to non-fast-forward, try to merge and push again
            if stderr.contains("non-fast-forward") || stderr.contains("rejected") {
                println!("Push rejected, pulling and merging remote changes...");
                
                // Pull with merge strategy, allowing unrelated histories
                let pull_merge_output = std::process::Command::new("git")
                    .args(&["pull", "--no-rebase", "--allow-unrelated-histories", "origin", &self.config.default_branch])
                    .current_dir(&repo_path)
                    .output()
                    .context("Failed to execute git pull for merge")?;
                
                if !pull_merge_output.status.success() {
                    let pull_stderr = String::from_utf8_lossy(&pull_merge_output.stderr);
                    return Err(anyhow::anyhow!("Failed to pull and merge: {}", pull_stderr));
                }
                
                // Try push again
                let retry_push_output = std::process::Command::new("git")
                    .args(&["push", "origin", &self.config.default_branch])
                    .current_dir(&repo_path)
                    .output()
                    .context("Failed to execute retry git push")?;
                
                if !retry_push_output.status.success() {
                    let retry_stderr = String::from_utf8_lossy(&retry_push_output.stderr);
                    return Err(anyhow::anyhow!("Failed to push after merge: {}", retry_stderr));
                }
                
                println!("Successfully merged and pushed changes");
            } else {
                return Err(anyhow::anyhow!("Failed to push to GitHub: {}", stderr));
            }
        } else {
            println!("Pushed changes to GitHub");
        }

        Ok(())
    }

    fn commit_repo_changes(&self, repo_path: &Path, message: &str) -> Result<()> {
        let git_repo = Repository::open(repo_path)
            .with_context(|| format!("Failed to open git repository: {:?}", repo_path))?;

        let signature = Signature::now("gitbox", "gitbox@local")
            .context("Failed to create git signature")?;

        let mut index = git_repo.index()
            .context("Failed to get git index")?;
        
        index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
            .context("Failed to add files to index")?;
        index.write()
            .context("Failed to write git index")?;

        let tree_id = index.write_tree()
            .context("Failed to write git tree")?;
        let tree = git_repo.find_tree(tree_id)
            .context("Failed to find git tree")?;

        let parent_commit = git_repo.head()
            .context("Failed to get HEAD")?
            .peel_to_commit()
            .context("Failed to peel to commit")?;

        git_repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &[&parent_commit],
        ).context("Failed to create commit")?;

        Ok(())
    }

    fn push_repo_changes(&self, repo_path: &Path) -> Result<()> {
        let push_output = std::process::Command::new("git")
            .args(&["push", "origin", "main"])
            .current_dir(repo_path)
            .output()
            .context("Failed to execute git push")?;

        if !push_output.status.success() {
            let stderr = String::from_utf8_lossy(&push_output.stderr);
            return Err(anyhow::anyhow!("Failed to push to remote: {}", stderr));
        }

        Ok(())
    }
}