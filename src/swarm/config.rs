use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Where a pane sits in the pipeline grid. Panes sharing the same (x, y) occupy one grid
/// cell and are tabbed between, rather than each getting their own screen space.
#[derive(Debug, Clone, Deserialize)]
pub struct PaneGridPos {
    pub grid_x: i32,
    pub grid_y: i32,
    /// Sub-pane labels for a tab whose cell holds more than one agent (e.g. a control-room
    /// tab tabbing between Mission Orchestrator / Intake Digest / ...). Empty for a plain
    /// one-agent-per-cell tab, where the tab's own key is the pane name.
    #[serde(default)]
    pub names: Vec<String>,
}

/// One role's real agent assignment: which CLI to launch, which model/effort to pass it,
/// which skills to mention, and what task to hand it when spawned.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentAssignment {
    pub pane: String,
    pub agent: String,
    pub model: String,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub task: Option<String>,
}

impl AgentAssignment {
    /// The task to hand this agent when spawned: the configured `task`, or a generic
    /// role-acknowledgement stub if none was given.
    pub fn effective_task(&self) -> String {
        self.task.clone().unwrap_or_else(|| {
            format!(
                "You are the {} agent in a multi-agent software dev swarm. \
                 Acknowledge your role in one short sentence.",
                self.pane
            )
        })
    }
}

fn default_turn_cap() -> usize {
    50
}

fn default_timeout_secs() -> u64 {
    180
}

fn default_max_turn_secs() -> u64 {
    1800
}

/// Hard limits on a swarm run to prevent runaway token expenditure.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
pub struct RunLimits {
    #[serde(default = "default_turn_cap")]
    pub turn_cap: usize,
    /// Idle limit: seconds a running turn may go without any output from the agent. The
    /// timer restarts on every event, so a turn that keeps streaming is not cut short.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// Hard cap in seconds on one whole turn, however much it streams.
    #[serde(default = "default_max_turn_secs")]
    pub max_turn_secs: u64,
    #[serde(default)]
    pub token_limit: Option<u64>,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            turn_cap: default_turn_cap(),
            timeout_secs: default_timeout_secs(),
            max_turn_secs: default_max_turn_secs(),
            token_limit: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwarmConfig {
    #[serde(default)]
    pub limits: RunLimits,
    pub tabs: HashMap<String, PaneGridPos>,
    pub agents: Vec<AgentAssignment>,
}

/// Default location of the swarm's tab/agent config: the single consolidated roster.
pub const DEFAULT_CONFIG_PATH: &str = "agents/full-software-factory/agents.yaml";

/// Project-local roster. A project can carry its own swarm here.
pub const PROJECT_CONFIG_PATH: &str = ".locust/agents.yaml";

/// The built-in roster, compiled into the binary so Locust works in any project directory.
pub const EMBEDDED_ROSTER: &str = include_str!("../../agents/full-software-factory/agents.yaml");

#[derive(Debug)]
pub enum ConfigError {
    Io(String),
    Parse(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(msg) => write!(f, "could not read config: {msg}"),
            ConfigError::Parse(msg) => write!(f, "could not parse config: {msg}"),
        }
    }
}

impl SwarmConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Io(e.to_string()))?;
        serde_yaml::from_str(&text).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Roster for the current project: its own `.locust/agents.yaml`, else the legacy
    /// `agents/full-software-factory/agents.yaml` in the project, else the built-in one.
    pub fn load_default() -> Result<Self, ConfigError> {
        for local in [PROJECT_CONFIG_PATH, DEFAULT_CONFIG_PATH] {
            if Path::new(local).is_file() {
                return Self::load(Path::new(local));
            }
        }
        serde_yaml::from_str(EMBEDDED_ROSTER).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Assignment for a given pane name, if one was configured.
    pub fn assignment_for(&self, pane: &str) -> Option<&AgentAssignment> {
        self.agents.iter().find(|a| a.pane == pane)
    }

    /// Every pane name the tabs define: the tab key for a plain tab, or each `names` entry.
    fn pane_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (key, pos) in &self.tabs {
            if pos.names.is_empty() {
                out.push(key.clone());
            } else {
                out.extend(pos.names.iter().cloned());
            }
        }
        out
    }

    /// Check the roster for mistakes that would otherwise fail late or silently. Returns every
    /// problem found, not just the first. Empty means the config is sound.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();

        if self.limits.turn_cap == 0 {
            problems.push("limits.turn_cap must be greater than 0".to_string());
        }
        if self.limits.timeout_secs == 0 {
            problems.push("limits.timeout_secs must be greater than 0".to_string());
        }
        if self.limits.max_turn_secs == 0 {
            problems.push("limits.max_turn_secs must be greater than 0".to_string());
        } else if self.limits.timeout_secs > self.limits.max_turn_secs {
            problems.push(format!(
                "limits.timeout_secs ({}) must not be larger than limits.max_turn_secs ({})",
                self.limits.timeout_secs, self.limits.max_turn_secs
            ));
        }
        if let Some(limit) = self.limits.token_limit
            && limit == 0
        {
            problems.push("limits.token_limit must be greater than 0".to_string());
        }

        let mut cells: HashMap<(i32, i32), Vec<&str>> = HashMap::new();
        for (key, pos) in &self.tabs {
            cells.entry((pos.grid_x, pos.grid_y)).or_default().push(key);
        }
        for ((x, y), mut keys) in cells {
            if keys.len() > 1 {
                keys.sort();
                problems.push(format!(
                    "grid cell ({x}, {y}) is used by more than one tab: {}",
                    keys.join(", ")
                ));
            }
        }

        let names = self.pane_names();
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for name in &names {
            if name.trim().is_empty() {
                problems.push("a pane has an empty name".to_string());
            }
            *seen.entry(name.as_str()).or_default() += 1;
        }
        for (name, count) in &seen {
            if *count > 1 {
                problems.push(format!("pane name \"{name}\" is defined {count} times"));
            }
        }

        let mut assigned: HashMap<&str, usize> = HashMap::new();
        for a in &self.agents {
            *assigned.entry(a.pane.as_str()).or_default() += 1;
            if crate::acp::manager::spec_for_agent_id(&a.agent).is_none() {
                problems.push(format!(
                    "agent \"{}\" for pane \"{}\" is not a known agent CLI",
                    a.agent, a.pane
                ));
            }
            if !names.iter().any(|n| n == &a.pane) {
                problems.push(format!(
                    "agent entry for pane \"{}\" matches no tab or sub-pane name",
                    a.pane
                ));
            }
        }
        for (pane, count) in &assigned {
            if *count > 1 {
                problems.push(format!("pane \"{pane}\" has {count} agent entries"));
            }
        }

        problems.sort();
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
tabs:
  User:
    grid_x: 1
    grid_y: 1
  Controller:
    grid_x: 2
    grid_y: 2

agents:
  - pane: User
    agent: claude
    model: opus-5.5
    effort: medium
    skills:
      - grill me
  - pane: Controller
    agent: claude
    model: claude-sonnet-5
    effort: medium
    skills: []
    task: "Acknowledge your role."
"#;

    #[test]
    fn test_parses_sample_config() {
        let config: SwarmConfig = serde_yaml::from_str(SAMPLE).unwrap();
        assert_eq!(config.tabs.len(), 2);
        assert_eq!(config.tabs["User"].grid_x, 1);
        assert_eq!(config.tabs["Controller"].grid_y, 2);
        assert!(config.tabs["User"].names.is_empty());
        assert_eq!(config.agents.len(), 2);
    }

    #[test]
    fn test_parses_names_list_on_a_tab() {
        const WITH_NAMES: &str = r#"
tabs:
  AGT-CTL:
    grid_x: 2
    grid_y: 1
    names:
      - Mission Orchestrator
      - Intake Digest

agents:
  - pane: "Mission Orchestrator"
    agent: claude
    model: claude-opus-5
    task: "Acknowledge your role."
  - pane: "Intake Digest"
    agent: claude
    model: claude-sonnet-5
    task: "Acknowledge your role."
"#;
        let config: SwarmConfig = serde_yaml::from_str(WITH_NAMES).unwrap();
        assert_eq!(
            config.tabs["AGT-CTL"].names,
            vec!["Mission Orchestrator", "Intake Digest"]
        );
        assert!(config.assignment_for("Mission Orchestrator").is_some());
    }

    #[test]
    fn test_effective_task_falls_back_when_absent() {
        let config: SwarmConfig = serde_yaml::from_str(SAMPLE).unwrap();
        let user = config.assignment_for("User").unwrap();
        assert!(user.effective_task().contains("User agent"));

        let controller = config.assignment_for("Controller").unwrap();
        assert_eq!(controller.effective_task(), "Acknowledge your role.");
    }

    #[test]
    fn test_assignment_lookup_misses_unknown_pane() {
        let config: SwarmConfig = serde_yaml::from_str(SAMPLE).unwrap();
        assert!(config.assignment_for("Nonexistent").is_none());
    }

    /// A tab with no `names` must have an assignment keyed by its own tab name; a tab with
    /// `names` must have one assignment per sub-pane, keyed by that bare sub-pane name.
    fn assert_every_tab_has_an_assignment(config: &SwarmConfig) {
        for (tab_name, pos) in &config.tabs {
            if pos.names.is_empty() {
                assert!(
                    config.assignment_for(tab_name).is_some(),
                    "tab {tab_name} has no agent assignment"
                );
            } else {
                for label in &pos.names {
                    assert!(
                        config.assignment_for(label).is_some(),
                        "sub-pane {label} (tab {tab_name}) has no agent assignment"
                    );
                }
            }
        }
    }

    #[test]
    fn test_default_config_file_parses() {
        // DEFAULT_CONFIG_PATH must stay pointed at the real, single consolidated roster.
        let config = SwarmConfig::load_default().expect("agents.yaml should parse");
        assert_eq!(config.tabs.len(), 5);
        assert_eq!(config.agents.len(), 21);
        assert_every_tab_has_an_assignment(&config);
    }

    #[test]
    fn test_full_factory_config_file_parses() {
        let config = SwarmConfig::load(Path::new("agents/full-software-factory/agents.yaml"))
            .expect("full-software-factory agents.yaml should parse");
        assert_eq!(config.tabs.len(), 5);
        assert_eq!(config.agents.len(), 21);
        assert_every_tab_has_an_assignment(&config);

        // Every tier row (grid_y 1..=2) should have at least one tab.
        for row in 1..=2 {
            assert!(
                config.tabs.values().any(|p| p.grid_y == row),
                "row {row} has no tabs"
            );
        }
    }

    #[test]
    fn test_test_swarm_config_files_parse() {
        for path in [
            "agents/test-swarm/agents.yaml",
            "agents/test-swarm.yaml",
        ] {
            let config = SwarmConfig::load(Path::new(path))
                .unwrap_or_else(|_| panic!("{path} should parse"));
            assert_eq!(config.tabs.len(), 5);
            assert_eq!(config.agents.len(), 21);
            assert_every_tab_has_an_assignment(&config);
            assert!(config.validate().is_empty(), "{:?}", config.validate());
            for agent in &config.agents {
                assert_eq!(agent.agent, "claude");
                assert_eq!(agent.model, "claude-haiku-3-5");
                assert_eq!(agent.effort.as_deref(), Some("low"));
            }
        }
    }
    fn cfg(yaml: &str) -> SwarmConfig {
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn test_validate_accepts_real_roster() {
        let config = SwarmConfig::load_default().unwrap();
        assert!(config.validate().is_empty(), "{:?}", config.validate());
    }

    #[test]
    fn test_validate_flags_duplicate_grid_cell() {
        let config = cfg("tabs:
  A: {grid_x: 1, grid_y: 1}
  B: {grid_x: 1, grid_y: 1}
agents:
  - {pane: A, agent: claude, model: m}
  - {pane: B, agent: claude, model: m}
");
        assert!(
            config
                .validate()
                .iter()
                .any(|p| p.contains("grid cell (1, 1)"))
        );
    }

    #[test]
    fn test_validate_flags_unknown_agent_and_orphan_pane() {
        let config = cfg("tabs:
  A: {grid_x: 1, grid_y: 1}
agents:
  - {pane: A, agent: nope, model: m}
  - {pane: Ghost, agent: claude, model: m}
");
        let problems = config.validate();
        assert!(problems.iter().any(|p| p.contains("not a known agent CLI")));
        assert!(problems.iter().any(|p| p.contains("matches no tab")));
    }

    #[test]
    fn test_validate_flags_duplicate_pane_and_assignment() {
        let config = cfg("tabs:
  A: {grid_x: 1, grid_y: 1, names: [X, X]}
agents:
  - {pane: X, agent: claude, model: m}
  - {pane: X, agent: claude, model: m}
");
        let problems = config.validate();
        assert!(problems.iter().any(|p| p.contains("defined 2 times")));
        assert!(problems.iter().any(|p| p.contains("2 agent entries")));
    }

    #[test]
    fn test_validate_flags_invalid_limits() {
        let config = cfg("limits: {turn_cap: 0, timeout_secs: 0, token_limit: 0}
tabs:
  A: {grid_x: 1, grid_y: 1}
agents:
  - {pane: A, agent: claude, model: m}
");
        let problems = config.validate();
        assert!(problems.iter().any(|p| p.contains("limits.turn_cap")));
        assert!(problems.iter().any(|p| p.contains("limits.timeout_secs")));
        assert!(problems.iter().any(|p| p.contains("limits.token_limit")));
    }
}
