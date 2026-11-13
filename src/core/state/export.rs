use super::*;

impl Model {
    /// Export all pane transcripts to `.locust/runs/<timestamp>/<pane>.md`.
    /// Secrets are always redacted before writing.
    pub fn export_transcripts(
        &self,
        base_dir: Option<&std::path::Path>,
    ) -> std::io::Result<std::path::PathBuf> {
        let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let target_dir = match base_dir {
            Some(dir) => dir.join(".locust").join("runs").join(&timestamp),
            None => std::path::Path::new(".locust")
                .join("runs")
                .join(&timestamp),
        };
        std::fs::create_dir_all(&target_dir)?;

        for pane in &self.swarm_panes {
            let filename = format!("{}.md", sanitize_filename(&pane.name));
            let filepath = target_dir.join(filename);
            let content = format!(
                "# Transcript: {}\n\n- **Agent**: {}\n- **Model**: {}\n- **Tab**: {}\n- **Status**: {:?}\n\n## Output\n\n{}\n",
                pane.name,
                pane.assignment.agent,
                pane.assignment.model,
                pane.tab,
                pane.status,
                crate::redact::redact(&pane.output)
            );
            write_private(&filepath, &content)?;
        }

        let controller_path = target_dir.join("Controller.md");
        let mut controller_content = String::from("# Transcript: Controller\n\n## History\n\n");
        for msg in &self.controller_log {
            controller_content.push_str(&format!(
                "- **[{:?}]**: {}\n",
                msg.role,
                crate::redact::redact(&msg.text)
            ));
        }
        write_private(&controller_path, &controller_content)?;

        Ok(target_dir)
    }
}
