pub mod repository;

use std::fs;
use std::path::PathBuf;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::{tool_handler, tool_router};

use crate::config::Config;
use crate::error::HooverError;

/// Run the MCP server on stdio transport.
pub async fn run_mcp_server(config: Config) -> crate::error::Result<()> {
    let service = HooverMcpService::new(config);

    let server = rmcp::ServiceExt::serve(service, rmcp::transport::io::stdio())
        .await
        .map_err(|e| HooverError::Other(format!("MCP server error: {e}")))?;

    server
        .waiting()
        .await
        .map_err(|e| HooverError::Other(format!("MCP server error: {e}")))?;

    Ok(())
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct SearchParams {
    #[schemars(description = "Text to search for")]
    query: String,
    #[schemars(description = "Start date (YYYY-MM-DD)")]
    from_date: Option<String>,
    #[schemars(description = "End date (YYYY-MM-DD)")]
    to_date: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct DateParam {
    #[schemars(description = "Date in YYYY-MM-DD format")]
    date: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct DateRangeParams {
    #[schemars(description = "Start date (YYYY-MM-DD)")]
    from: String,
    #[schemars(description = "End date (YYYY-MM-DD)")]
    to: String,
}

#[derive(Clone, Debug)]
struct HooverMcpService {
    output_dir: PathBuf,
    config: Config,
    tool_router: ToolRouter<Self>,
}

impl HooverMcpService {
    fn new(config: Config) -> Self {
        let output_dir = Config::expand_path(&config.output.directory);
        Self {
            output_dir,
            config,
            tool_router: Self::tool_router(),
        }
    }

    fn list_markdown_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.output_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    files.push(path);
                }
            }
        }
        files.sort();
        files
    }
}

#[tool_router]
impl HooverMcpService {
    #[rmcp::tool(description = "Search across transcription files for a query string")]
    fn search_transcriptions(
        &self,
        Parameters(SearchParams {
            query,
            from_date,
            to_date,
        }): Parameters<SearchParams>,
    ) -> String {
        let files = self.list_markdown_files();
        let mut results = Vec::new();

        for file in files {
            let filename = file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();

            // Filter by date range
            if let Some(ref from) = from_date
                && filename < from.as_str()
            {
                continue;
            }
            if let Some(ref to) = to_date
                && filename > to.as_str()
            {
                continue;
            }

            if let Ok(content) = fs::read_to_string(&file) {
                for (i, line) in content.lines().enumerate() {
                    if line.to_lowercase().contains(&query.to_lowercase()) {
                        results.push(format!("{}:{}: {}", filename, i + 1, line));
                    }
                }
            }
        }

        if results.is_empty() {
            "No matches found.".to_string()
        } else {
            results.join("\n")
        }
    }

    #[rmcp::tool(description = "Get the full transcription for a specific day")]
    fn get_day(&self, Parameters(DateParam { date }): Parameters<DateParam>) -> String {
        let path = self.output_dir.join(format!("{date}.md"));
        fs::read_to_string(&path).unwrap_or_else(|_| format!("No transcription found for {date}"))
    }

    #[rmcp::tool(description = "List all available transcription dates")]
    fn list_dates(&self) -> String {
        let files = self.list_markdown_files();
        let dates: Vec<&str> = files
            .iter()
            .filter_map(|f| f.file_stem().and_then(|s| s.to_str()))
            .collect();

        if dates.is_empty() {
            "No transcriptions found.".to_string()
        } else {
            dates.join("\n")
        }
    }

    #[rmcp::tool(description = "Get transcriptions for a date range")]
    fn get_date_range(
        &self,
        Parameters(DateRangeParams { from, to }): Parameters<DateRangeParams>,
    ) -> String {
        let files = self.list_markdown_files();
        let mut content = Vec::new();

        for file in files {
            let filename = file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();

            if filename >= from.as_str()
                && filename <= to.as_str()
                && let Ok(text) = fs::read_to_string(&file)
            {
                content.push(text);
            }
        }

        if content.is_empty() {
            format!("No transcriptions found between {from} and {to}")
        } else {
            content.join("\n---\n\n")
        }
    }

    #[rmcp::tool(description = "Get summary statistics about transcriptions")]
    fn get_summary(&self) -> String {
        let files = self.list_markdown_files();
        let dates: Vec<&str> = files
            .iter()
            .filter_map(|f| f.file_stem().and_then(|s| s.to_str()))
            .collect();

        let total_entries: usize = files
            .iter()
            .map(|f| {
                fs::read_to_string(f)
                    .map(|c| c.lines().filter(|l| l.starts_with("**[")).count())
                    .unwrap_or(0)
            })
            .sum();

        let first = dates.first().copied().unwrap_or("none");
        let last = dates.last().copied().unwrap_or("none");

        format!(
            "Days: {}\nEntries: {total_entries}\nFirst: {first}\nLast: {last}",
            dates.len()
        )
    }

    #[rmcp::tool(description = "List enrolled speaker profiles")]
    fn get_speakers(&self) -> String {
        let profiles_dir = Config::expand_path(&self.config.speaker.profiles_dir);
        if !profiles_dir.exists() {
            return "No speaker profiles directory found.".to_string();
        }

        let mut names = Vec::new();
        if let Ok(entries) = fs::read_dir(&profiles_dir) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|e| e.to_str()) == Some("bin")
                    && let Some(name) = entry.path().file_stem().and_then(|s| s.to_str())
                {
                    names.push(name.to_string());
                }
            }
        }

        if names.is_empty() {
            "No speaker profiles enrolled.".to_string()
        } else {
            names.sort();
            names.join("\n")
        }
    }
}

#[tool_handler]
impl rmcp::ServerHandler for HooverMcpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Hoover transcription data server. Query daily transcriptions, search across dates, and view speaker profiles.".into()),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            ..Default::default()
        }
    }
}
