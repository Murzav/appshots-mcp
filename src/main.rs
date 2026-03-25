use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "appshots-mcp",
    about = "MCP server for ASO-optimized App Store screenshots",
    version
)]
struct Cli {
    /// Path to the target app project directory
    #[arg(long, default_value = ".")]
    project_dir: PathBuf,

    /// Path to the shared glossary file
    #[arg(long, default_value = "glossary.json")]
    glossary_path: PathBuf,

    /// Path to the appshots project config
    #[arg(long, default_value = "appshots.json")]
    config_path: PathBuf,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let store = Arc::new(
        appshots_mcp::io::fs::FsFileStore::new().with_project_dir(cli.project_dir.clone()),
    );
    let server = appshots_mcp::server::AppShotsMcpServer::new(
        store,
        cli.project_dir,
        cli.glossary_path,
        cli.config_path,
    );

    let transport = rmcp::transport::io::stdio();

    match server.serve(transport).await {
        Ok(service) => {
            if let Err(e) = service.waiting().await {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_default_values() {
        let cli = Cli::try_parse_from(["appshots-mcp"]).unwrap();
        assert_eq!(cli.project_dir, PathBuf::from("."));
        assert_eq!(cli.glossary_path, PathBuf::from("glossary.json"));
        assert_eq!(cli.config_path, PathBuf::from("appshots.json"));
    }

    #[test]
    fn cli_custom_values() {
        let cli = Cli::try_parse_from([
            "appshots-mcp",
            "--project-dir",
            "/my/project",
            "--glossary-path",
            "/my/glossary.json",
            "--config-path",
            "/my/config.json",
        ])
        .unwrap();
        assert_eq!(cli.project_dir, PathBuf::from("/my/project"));
        assert_eq!(cli.glossary_path, PathBuf::from("/my/glossary.json"));
        assert_eq!(cli.config_path, PathBuf::from("/my/config.json"));
    }

    #[test]
    fn cli_rejects_unknown_flag() {
        let result = Cli::try_parse_from(["appshots-mcp", "--unknown"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_version_flag() {
        let result = Cli::try_parse_from(["appshots-mcp", "--version"]);
        // --version causes clap to exit with an error containing the version
        assert!(result.is_err());
    }
}
