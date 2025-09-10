use clap::{Parser, Subcommand};
use anyhow::Result;

mod config;
mod repo;
mod github;
mod sync;

use config::Config;
use repo::RepoManager;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new repository
    AddRepo {
        /// Repository name
        name: String,
    },
    /// Delete a local repository
    DeleteLocalRepo {
        /// Repository name to delete
        #[arg(long)]
        get: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Delete a local repository (alias for delete-local-repo)
    #[command(name = "remove-local-repo")]
    RemoveLocalRepo {
        /// Repository name to delete
        #[arg(long)]
        get: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Sync a file to a repository
    Sync {
        /// File or directory to sync
        path: String,
        /// Target repository (defaults to 'gitbox-default')
        #[arg(long)]
        repo: Option<String>,
    },
    /// Sync a file from remote repository to current directory
    #[command(name = "sync-from-remote")]
    SyncFromRemote {
        /// File name to sync from remote
        filename: String,
        /// Source repository (defaults to 'gitbox-default')
        #[arg(long)]
        repo: Option<String>,
    },
    /// List all repositories
    ListRepos,
    /// List all synced files across repositories
    ListFiles,
    /// List remote files in the default repository
    #[command(name = "list-remote-files")]
    ListRemoteFiles,
    /// Push local file changes to remote repository
    #[command(name = "sync-push")]
    SyncPush {
        /// File to push (optional - if not specified, pushes all changes)
        file: Option<String>,
        /// Target repository (defaults to 'gitbox-default')
        #[arg(long)]
        repo: Option<String>,
    },
    /// Pull remote file from repository to current directory
    #[command(name = "sync-pull")]
    SyncPull {
        /// File to pull from remote
        file: String,
        /// Source repository (defaults to 'gitbox-default')
        #[arg(long)]
        repo: Option<String>,
    },
    /// Sync all repositories with remotes
    SyncAllRepos,
    /// Repository operations
    Repo {
        /// Get repository by name
        #[arg(long)]
        get: String,
        #[command(subcommand)]
        action: RepoAction,
    },
}

#[derive(Subcommand)]
enum RepoAction {
    /// List files in the repository
    List,
    /// Show repository information
    Info,
    /// Sync repository with GitHub (pull/push)
    Sync,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    let config = Config::load_or_create()?;
    let mut repo_manager = RepoManager::new(&config)?;

    match cli.command {
        Commands::AddRepo { name } => {
            repo_manager.add_repo(&name).await?;
            println!("Repository '{}' created and pushed to GitHub", name);
        }
        Commands::DeleteLocalRepo { get, force } => {
            repo_manager.delete_repo(&get, force)?;
        }
        Commands::RemoveLocalRepo { get, force } => {
            repo_manager.delete_repo(&get, force)?;
        }
        Commands::Sync { path, repo } => {
            let repo_name = repo.unwrap_or_else(|| "gitbox-default".to_string());
            repo_manager.sync_file_with_default(&path, &repo_name).await?;
            println!("File '{}' synced to repository '{}' and pushed to GitHub", path, repo_name);
        }
        Commands::SyncFromRemote { filename, repo } => {
            let repo_name = repo.unwrap_or_else(|| "gitbox-default".to_string());
            repo_manager.sync_from_remote(&filename, &repo_name).await?;
            println!("File '{}' synced from repository '{}' to current directory", filename, repo_name);
        }
        Commands::ListRepos => {
            let repos = repo_manager.list_repos()?;
            if repos.is_empty() {
                println!("No repositories found");
            } else {
                println!("Repositories:");
                for repo in repos {
                    println!("  {}", repo);
                }
            }
        }
        Commands::ListFiles => {
            let files = repo_manager.list_all_synced_files()?;
            if files.is_empty() {
                println!("No files are currently being synced");
            } else {
                println!("Synced files ({} total):", files.len());
                for file in files {
                    println!("  {} -> {}", file.original_path, file.repository);
                }
            }
        }
        Commands::ListRemoteFiles => {
            let files = repo_manager.list_remote_files().await?;
            if files.is_empty() {
                println!("No files found in remote repository 'gitbox-default'");
            } else {
                println!("Remote files in 'gitbox-default' ({} total):", files.len());
                for file in files {
                    println!("  {}", file);
                }
            }
        }
        Commands::SyncPush { file, repo } => {
            let repo_name = repo.unwrap_or_else(|| "gitbox-default".to_string());
            repo_manager.sync_push(&repo_name, file.as_deref()).await?;
            if let Some(file_name) = file {
                println!("Successfully pushed file '{}' to repository '{}'", file_name, repo_name);
            } else {
                println!("Successfully pushed local changes to repository '{}'", repo_name);
            }
        }
        Commands::SyncPull { file, repo } => {
            let repo_name = repo.unwrap_or_else(|| "gitbox-default".to_string());
            repo_manager.sync_pull(&repo_name, &file).await?;
            println!("Successfully pulled file '{}' from repository '{}'", file, repo_name);
        }
        Commands::SyncAllRepos => {
            let repos = repo_manager.list_repos()?;
            if repos.is_empty() {
                println!("No repositories found to sync");
            } else {
                println!("Syncing {} repositories with remotes...", repos.len());
                for repo in repos {
                    match repo_manager.sync_repo(&repo) {
                        Ok(_) => println!("✓ Synced '{}'", repo),
                        Err(e) => println!("✗ Failed to sync '{}': {}", repo, e),
                    }
                }
                println!("Sync completed");
            }
        }
        Commands::Repo { get, action } => {
            match action {
                RepoAction::List => {
                    let files = repo_manager.list_repo_files(&get)?;
                    if files.is_empty() {
                        println!("No files in repository '{}'", get);
                    } else {
                        println!("Files in repository '{}':", get);
                        for file in files {
                            println!("  {}", file);
                        }
                    }
                }
                RepoAction::Info => {
                    let info = repo_manager.get_repo_info(&get)?;
                    println!("{}", info);
                }
                RepoAction::Sync => {
                    repo_manager.sync_repo(&get)?;
                    println!("Repository '{}' synced with GitHub", get);
                }
            }
        }
    }

    Ok(())
}