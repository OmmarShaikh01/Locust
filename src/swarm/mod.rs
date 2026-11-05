pub mod config;

pub use config::{AgentAssignment, PaneGridPos, RunLimits, SwarmConfig};

/// Result of one real agent run against a Swarm pane's task, over its shared ACP
/// connection (see `acp::Manager::run_one_shot`).
pub struct RunOutcome {
    pub pane: String,
    pub output: Result<String, String>,
    pub usage: Option<crate::acp::UsageInfo>,
}

impl RunOutcome {
    pub fn new(pane: impl Into<String>, output: Result<String, String>) -> Self {
        Self {
            pane: pane.into(),
            output,
            usage: None,
        }
    }

    pub fn with_usage(mut self, usage: Option<crate::acp::UsageInfo>) -> Self {
        self.usage = usage;
        self
    }
}

/// One agent-to-agent message an agent addressed to another pane from inside its own reply.
/// This is the entire addressing convention: agents choose who they talk to by writing these
/// themselves, there is no separate human-facing send command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    pub target: String,
    pub message: String,
}

/// Parse every `@[Target Pane]: message` directive out of an agent's reply text, one per line.
/// Brackets are required (pane names contain spaces), the colon must follow immediately, and
/// both the target and the message must be non-empty after trimming.
///
/// Lines inside a ``` code fence and lines starting with `>` are ignored. An agent that quotes
/// a file or a web page it read would otherwise hand the quoted text's `@[...]` lines to
/// other agents as if it had written them itself.
pub fn parse_directives(text: &str) -> Vec<Directive> {
    let mut in_fence = false;
    text.lines()
        .filter(|line| {
            let t = line.trim_start();
            if t.starts_with("```") || t.starts_with("~~~") {
                in_fence = !in_fence;
                return false;
            }
            !in_fence && !t.starts_with('>')
        })
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("@[")?;
            let (target, after) = rest.split_once(']')?;
            let message = after.strip_prefix(':')?.trim();
            let target = target.trim();
            if target.is_empty() || message.is_empty() {
                return None;
            }
            Some(Directive {
                target: target.to_string(),
                message: message.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_directives_single() {
        let directives = parse_directives("@[Spec Agent]: please draft the SPEC.md outline");
        assert_eq!(
            directives,
            vec![Directive {
                target: "Spec Agent".to_string(),
                message: "please draft the SPEC.md outline".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_directives_multiple_lines() {
        let text = "Acknowledged.\n@[Spec Agent]: draft the spec\nSome other text\n@[Security Sweeper]: audit this PR";
        let directives = parse_directives(text);
        assert_eq!(directives.len(), 2);
        assert_eq!(directives[0].target, "Spec Agent");
        assert_eq!(directives[1].target, "Security Sweeper");
    }

    #[test]
    fn test_parse_directives_ignores_plain_text() {
        assert!(parse_directives("just a normal reply with no directives").is_empty());
    }

    #[test]
    fn test_parse_directives_requires_brackets() {
        assert!(parse_directives("@Spec Agent: no brackets here").is_empty());
    }

    #[test]
    fn test_parse_directives_rejects_empty_target_or_message() {
        assert!(parse_directives("@[]: message with empty target").is_empty());
        assert!(parse_directives("@[Spec Agent]:   ").is_empty());
    }

    #[test]
    fn test_parse_directives_ignores_code_fences_and_quotes() {
        let text = "Here is the file:\n```\n@[Spec Agent]: injected from a file\n```\n> @[Security Sweeper]: quoted\n@[Architecture Agent]: real one";
        let directives = parse_directives(text);
        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].target, "Architecture Agent");
    }

    #[test]
    fn test_parse_directives_unclosed_fence_hides_the_rest() {
        let text = "```\n@[Spec Agent]: inside an unclosed fence";
        assert!(parse_directives(text).is_empty());
    }

    #[test]
    fn test_parse_directives_trims_whitespace() {
        let directives = parse_directives("  @[ Spec Agent ]:   draft it   ");
        assert_eq!(directives[0].target, "Spec Agent");
        assert_eq!(directives[0].message, "draft it");
    }

    #[test]
    fn test_parse_directives_wildcard_and_tab() {
        let text = "@[*]: broadcast notice\n@[AGT-CTL]: tab directive";
        let directives = parse_directives(text);
        assert_eq!(directives.len(), 2);
        assert_eq!(directives[0].target, "*");
        assert_eq!(directives[0].message, "broadcast notice");
        assert_eq!(directives[1].target, "AGT-CTL");
        assert_eq!(directives[1].message, "tab directive");
    }
}
