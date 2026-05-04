use crate::agents::AgentIntegration;
use colored::*;
use serde_yaml::Value as YamlValue;
use std::fs;
use std::path::PathBuf;

pub struct HermesIntegration;

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

fn hermes_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hermes/config.yaml")
}

fn hermes_plugin_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hermes/plugins/omni-signal-engine")
}

// ---------------------------------------------------------------------------
// YAML helpers (public for tests)
// ---------------------------------------------------------------------------

/// Patch `~/.hermes/config.yaml` to add omni under `mcp_servers:`.
/// Creates the file if missing. Idempotent.
pub fn patch_hermes_config(path: &PathBuf, exe_path: &str) -> anyhow::Result<()> {
    let content = if path.exists() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };

    let mut doc: serde_yaml::Mapping = if content.trim().is_empty() {
        serde_yaml::Mapping::new()
    } else {
        serde_yaml::from_str::<serde_yaml::Mapping>(&content)
            .unwrap_or_else(|_| serde_yaml::Mapping::new())
    };

    // Ensure mcp_servers exists
    let mcp_key = YamlValue::String("mcp_servers".to_string());
    if !doc.contains_key(&mcp_key) {
        doc.insert(
            mcp_key.clone(),
            YamlValue::Mapping(serde_yaml::Mapping::new()),
        );
    }

    let mcp_servers = doc
        .get_mut(&mcp_key)
        .and_then(|v| v.as_mapping_mut())
        .ok_or_else(|| anyhow::anyhow!("mcp_servers is not a mapping"))?;

    let omni_key = YamlValue::String("omni".to_string());
    if mcp_servers.contains_key(&omni_key) {
        // Already present — update the command in case the binary moved
        if let Some(entry) = mcp_servers
            .get_mut(&omni_key)
            .and_then(|v| v.as_mapping_mut())
        {
            entry.insert(
                YamlValue::String("command".to_string()),
                YamlValue::String(exe_path.to_string()),
            );
        }
    } else {
        let mut omni_entry = serde_yaml::Mapping::new();
        omni_entry.insert(
            YamlValue::String("command".to_string()),
            YamlValue::String(exe_path.to_string()),
        );
        omni_entry.insert(
            YamlValue::String("args".to_string()),
            YamlValue::Sequence(vec![YamlValue::String("--mcp".to_string())]),
        );
        mcp_servers.insert(omni_key, YamlValue::Mapping(omni_entry));
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let new_content = serde_yaml::to_string(&doc)?;
    fs::write(path, new_content)?;
    Ok(())
}

/// Remove the `omni` entry from `mcp_servers` in the Hermes config.
/// No-op if the file doesn't exist or the entry isn't present.
pub fn remove_omni_from_hermes_config(path: &PathBuf) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(());
    }

    let mut doc: serde_yaml::Mapping = match serde_yaml::from_str(&content) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };

    let mcp_key = YamlValue::String("mcp_servers".to_string());
    if let Some(mcp_servers) = doc.get_mut(&mcp_key).and_then(|v| v.as_mapping_mut()) {
        let omni_key = YamlValue::String("omni".to_string());
        mcp_servers.remove(&omni_key);
    }

    let new_content = serde_yaml::to_string(&doc)?;
    fs::write(path, new_content)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// AgentIntegration impl
// ---------------------------------------------------------------------------

impl AgentIntegration for HermesIntegration {
    fn id(&self) -> &'static str {
        "hermes"
    }

    fn name(&self) -> &'static str {
        "Hermes Agent"
    }

    fn install(&self, exe_path: &str) -> anyhow::Result<()> {
        // 1. Patch ~/.hermes/config.yaml
        let config_path = hermes_config_path();
        patch_hermes_config(&config_path, exe_path)?;
        println!(
            "  {} {} registered in ~/.hermes/config.yaml",
            "✓".green(),
            "MCP Server".bold()
        );

        // 2. Install Python plugin
        let dest = hermes_plugin_dir();
        fs::create_dir_all(&dest)?;

        println!(
            "  {} Downloading Hermes plugin files from GitHub...",
            "↓".cyan()
        );

        for file in &["__init__.py", "plugin.yaml"] {
            let url = format!(
                "https://raw.githubusercontent.com/tomraider4720/omni/main/integrations/hermes/{}",
                file
            );
            let to = dest.join(file);

            let response = ureq::get(&url)
                .call()
                .map_err(|e| anyhow::anyhow!("Failed to download {}: {}", file, e))?;
            let mut dest_file = fs::File::create(&to)?;
            std::io::copy(&mut response.into_reader(), &mut dest_file)?;
        }

        println!(
            "  {} {} installed to ~/.hermes/plugins/omni-signal-engine/",
            "✓".green(),
            "Plugin".bold()
        );

        Ok(())
    }

    fn uninstall(&self) -> anyhow::Result<()> {
        let config_path = hermes_config_path();
        remove_omni_from_hermes_config(&config_path)?;
        println!(
            "  {} {} removed from ~/.hermes/config.yaml",
            "✓".yellow(),
            "MCP Server".bold()
        );

        let dest = hermes_plugin_dir();
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
            println!(
                "  {} {} removed from ~/.hermes/plugins/",
                "✓".yellow(),
                "Plugin".bold()
            );
        }

        Ok(())
    }

    fn doctor_check(&self, fix_mode: bool, warnings: &mut Vec<String>) -> bool {
        let mut all_ok = true;

        println!("\n  {}", "Hermes Agent:".cyan());

        // Check MCP server in config.yaml
        let config_path = hermes_config_path();
        let mcp_ok = if config_path.exists() {
            let content = fs::read_to_string(&config_path).unwrap_or_default();
            content.contains("command:") && content.contains("--mcp") && content.contains("omni:")
        } else {
            false
        };

        if mcp_ok {
            println!(
                "   {:<15} {} {}",
                "MCP Server:".bright_black(),
                "~/.hermes/config.yaml".bright_black(),
                "[OK]".green().bold()
            );
        } else if fix_mode {
            if let Ok(exe_path) = std::env::current_exe() {
                let _ = patch_hermes_config(&config_path, &exe_path.to_string_lossy());
            }
            println!(
                "   {:<15} {}",
                "MCP Server:".bright_black(),
                "[FIXED] added to config.yaml".green().bold()
            );
        } else {
            println!(
                "   {:<15} {}",
                "MCP Server:".bright_black(),
                "[WARNING] not configured".yellow().bold()
            );
            warnings
                .push("Hermes MCP server not configured. Run `omni install hermes`.".to_string());
            all_ok = false;
        }

        // Check plugin files
        let plugin_dir = hermes_plugin_dir();
        let plugin_ok = plugin_dir.join("__init__.py").exists();

        if plugin_ok {
            println!(
                "   {:<15} {} {}",
                "Plugin:".bright_black(),
                "~/.hermes/plugins/omni-signal-engine/".bright_black(),
                "[OK]".green().bold()
            );
        } else if fix_mode {
            if let Ok(exe_path) = std::env::current_exe() {
                let _ = self.install(&exe_path.to_string_lossy());
            }
            println!(
                "   {:<15} {}",
                "Plugin:".bright_black(),
                "[FIXED] installed".green().bold()
            );
        } else {
            println!(
                "   {:<15} {}",
                "Plugin:".bright_black(),
                "[WARNING] not installed".yellow()
            );
            warnings.push("Hermes plugin not installed. Run `omni install hermes`.".to_string());
            all_ok = false;
        }

        all_ok
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_config(content: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn test_id_and_name() {
        let h = HermesIntegration;
        assert_eq!(h.id(), "hermes");
        assert_eq!(h.name(), "Hermes Agent");
    }

    #[test]
    fn test_patch_yaml_adds_mcp_entry_when_mcp_servers_missing() {
        let content = "model:\n  default: claude\n";
        let (_dir, path) = make_config(content);
        patch_hermes_config(&path, "/usr/bin/omni").unwrap();
        let result = fs::read_to_string(&path).unwrap();
        assert!(result.contains("mcp_servers"), "must contain mcp_servers");
        assert!(result.contains("omni"), "must contain omni entry");
        assert!(result.contains("--mcp"), "must contain --mcp arg");
    }

    #[test]
    fn test_patch_yaml_adds_mcp_entry_when_mcp_servers_exists_without_omni() {
        let content = "mcp_servers:\n  filesystem:\n    command: npx\n";
        let (_dir, path) = make_config(content);
        patch_hermes_config(&path, "/usr/bin/omni").unwrap();
        let result = fs::read_to_string(&path).unwrap();
        assert!(
            result.contains("filesystem"),
            "must preserve existing entries"
        );
        assert!(result.contains("omni"), "must add omni entry");
    }

    #[test]
    fn test_patch_yaml_idempotent() {
        let content = "mcp_servers:\n  filesystem:\n    command: npx\n";
        let (_dir, path) = make_config(content);
        patch_hermes_config(&path, "/usr/bin/omni").unwrap();
        let after_first = fs::read_to_string(&path).unwrap();
        patch_hermes_config(&path, "/usr/bin/omni").unwrap();
        let after_second = fs::read_to_string(&path).unwrap();
        assert_eq!(
            after_first,
            after_second,
            "patching twice must be idempotent"
        );
    }

    #[test]
    fn test_remove_mcp_entry_removes_omni_keeps_others() {
        let content = "mcp_servers:\n  filesystem:\n    command: npx\n  omni:\n    command: /usr/bin/omni\n    args:\n      - --mcp\n";
        let (_dir, path) = make_config(content);
        remove_omni_from_hermes_config(&path).unwrap();
        let result = fs::read_to_string(&path).unwrap();
        assert!(!result.contains("command: /usr/bin/omni"), "omni entry must be gone");
        assert!(result.contains("filesystem"), "other entries must be preserved");
    }

    #[test]
    fn test_remove_mcp_entry_noop_when_not_present() {
        let content = "mcp_servers:\n  filesystem:\n    command: npx\n";
        let (_dir, path) = make_config(content);
        remove_omni_from_hermes_config(&path).unwrap();
        let result = fs::read_to_string(&path).unwrap();
        assert!(result.contains("filesystem"), "other entries preserved");
    }

    #[test]
    fn test_remove_noop_when_file_missing() {
        let path = PathBuf::from("/nonexistent/config.yaml");
        let _ = remove_omni_from_hermes_config(&path);
        // should not panic
    }

    #[test]
    fn test_patch_empty_file() {
        let (_dir, path) = make_config("");
        patch_hermes_config(&path, "/usr/bin/omni").unwrap();
        let result = fs::read_to_string(&path).unwrap();
        assert!(result.contains("mcp_servers"));
        assert!(result.contains("omni"));
    }
}
