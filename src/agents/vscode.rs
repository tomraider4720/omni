use crate::agents::AgentIntegration;
use colored::*;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

pub struct VscodeIntegration;

impl VscodeIntegration {
    /// Returns the path to .vscode/mcp.json in the current working directory.
    fn config_path() -> PathBuf {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".vscode/mcp.json")
    }
}

impl AgentIntegration for VscodeIntegration {
    fn id(&self) -> &'static str {
        "vscode"
    }

    fn name(&self) -> &'static str {
        "VS Code"
    }

    fn install(&self, exe_path: &str) -> anyhow::Result<()> {
        let config_path = Self::config_path();

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut val = if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
        } else {
            json!({})
        };

        // VSCode uses "servers" (not "mcpServers") per MCP spec
        if let Some(obj) = val.as_object_mut() {
            let servers = obj.entry("servers").or_insert_with(|| json!({}));
            if let Some(servers_obj) = servers.as_object_mut() {
                servers_obj.insert(
                    "omni".to_string(),
                    json!({
                        "command": exe_path,
                        "args": ["--mcp"],
                        "env": {
                            "OMNI_AGENT_ID": "vscode"
                        }
                    }),
                );
            }
        }

        fs::write(&config_path, serde_json::to_string_pretty(&val)?)?;
        println!(
            "  {} Configured MCP Server in .vscode/mcp.json",
            "✓".green()
        );
        Ok(())
    }

    fn uninstall(&self) -> anyhow::Result<()> {
        let config_path = Self::config_path();

        if !config_path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&config_path)?;
        let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&content) else {
            return Ok(());
        };

        if let Some(obj) = val.as_object_mut()
            && let Some(servers) = obj.get_mut("servers").and_then(|v| v.as_object_mut())
        {
            servers.remove("omni");
        }

        fs::write(&config_path, serde_json::to_string_pretty(&val)?)?;
        println!(
            "  {} Removed MCP Server from .vscode/mcp.json",
            "✓".yellow()
        );
        Ok(())
    }

    fn doctor_check(&self, _fix_mode: bool, _warnings: &mut Vec<String>) -> bool {
        let config_path = Self::config_path();

        println!("\n  {}", "VS Code:".cyan());
        if config_path.exists()
            && fs::read_to_string(&config_path)
                .unwrap_or_default()
                .contains("\"omni\"")
        {
            println!(
                "   {:<15} {} {}",
                "Config:".bright_black(),
                ".vscode/mcp.json".bright_black(),
                "[OK]".green().bold()
            );
            true
        } else {
            println!(
                "   {:<15} {}",
                "Config:".bright_black(),
                "not configured".bright_black()
            );
            true
        }
    }
}
