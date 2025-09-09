# GitBox

A Rust CLI tool for syncing files across computers using Git and GitHub.

## Overview

GitBox helps you manage and synchronize files across multiple machines by creating Git repositories that are automatically pushed to GitHub as private repositories. It uses symbolic links to keep files in their original locations while tracking them in centralized repositories.

## Features

- **Repository Management**: Create and manage Git repositories in `~/.gitbox/repos/`
- **File Syncing**: Sync files and directories to repositories using symbolic links
- **GitHub Integration**: Automatically create private GitHub repositories
- **Metadata Tracking**: Track synced files with `.gitbox` metadata files
- **Cross-platform**: Works on Linux, macOS, and Windows

## Installation

### From Source

```bash
git clone <repository-url>
cd gitbox
cargo install --path .
```

## Configuration

GitBox stores its configuration in `~/.gitbox/config.toml` and uses the GitHub CLI (`gh`) for GitHub operations.

### Prerequisites

1. **Install GitHub CLI**: 
   ```bash
   # Ubuntu/Debian
   sudo apt install gh
   
   # macOS
   brew install gh
   
   # Or download from: https://github.com/cli/cli/releases
   ```

2. **Authenticate with GitHub**:
   ```bash
   gh auth login
   ```

That's it! No tokens to manage - GitBox uses your existing `gh` authentication.

## Usage

### Create a New Repository

```bash
gitbox add-repo my-repo
```

This will:
- Create a local Git repository at `~/.gitbox/repos/my-repo`
- Create a private GitHub repository
- Initialize with a `.gitbox` metadata file
- Push to GitHub

### Sync a File to a Repository

```bash
# From any directory
gitbox sync file.txt --repo=my-repo
```

This will:
- Create a symbolic link from the repository to your file
- Update metadata files (both local `.gitbox` and repository `.gitbox`)
- Commit and push changes to GitHub

### List Repositories

```bash
gitbox list-repos
```

### List Files in a Repository

```bash
gitbox repo --get=my-repo list
```

## File Structure

```
~/.gitbox/
├── config.toml          # Global configuration
└── repos/               # All managed repositories
    └── my-repo/         # Individual repository
        ├── .git/        # Git repository data
        ├── .gitbox      # Repository metadata
        └── files/       # Synced files (as symlinks)
```

## Workflow Example

1. **Setup**: Create a repository for your project configs
   ```bash
   gitbox add-repo dotfiles
   ```

2. **Sync files**: Add your configuration files
   ```bash
   cd ~/
   gitbox sync .vimrc --repo=dotfiles
   gitbox sync .bashrc --repo=dotfiles
   ```

3. **On another machine**: Clone the repository and the symlinks will point to the synced files
   ```bash
   # The repository is already on GitHub, symlinks maintain file locations
   ```

## Development

### Build

```bash
cargo build
```

### Run Tests

```bash
cargo test
```

### Run with Debug Logs

```bash
RUST_LOG=debug cargo run -- add-repo test-repo
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

## License

MIT License - see LICENSE file for details