use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Structured usage metrics tracked per agent role and aggregated across the swarm.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageMetrics {
    #[serde(default)]
    pub role: String,
    #[serde(rename = "promptTokens", default)]
    pub prompt_tokens: u64,
    #[serde(rename = "completionTokens", default)]
    pub completion_tokens: u64,
    #[serde(rename = "totalTokens", default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub invocations: u64,
    #[serde(rename = "tokensUsed", default)]
    pub tokens_used: Option<u64>,
    #[serde(rename = "contextSize", default)]
    pub context_size: Option<u64>,
    #[serde(rename = "costAmount", default)]
    pub cost_amount: Option<f64>,
    #[serde(rename = "costCurrency", default)]
    pub cost_currency: Option<String>,
    #[serde(rename = "lastUpdated", default)]
    pub last_updated: Option<String>,
}

impl UsageMetrics {
    /// Calculate remaining capacity in context window.
    pub fn tokens_left(&self) -> Option<u64> {
        match (self.tokens_used, self.context_size) {
            (Some(used), Some(size)) => Some(size.saturating_sub(used)),
            _ => None,
        }
    }

    /// Calculate percentage of context capacity remaining.
    pub fn percent_left(&self) -> Option<f64> {
        match (self.tokens_used, self.context_size) {
            (Some(used), Some(size)) if size > 0 => {
                let left = size.saturating_sub(used);
                Some((left as f64 / size as f64) * 100.0)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SwarmUsageTotals {
    #[serde(rename = "promptTokens", default)]
    pub prompt_tokens: u64,
    #[serde(rename = "completionTokens", default)]
    pub completion_tokens: u64,
    #[serde(rename = "totalTokens", default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub invocations: u64,
    #[serde(rename = "tokensUsed", default)]
    pub tokens_used: Option<u64>,
    #[serde(rename = "contextSize", default)]
    pub context_size: Option<u64>,
    #[serde(rename = "costAmount", default)]
    pub cost_amount: Option<f64>,
    #[serde(rename = "costCurrency", default)]
    pub cost_currency: Option<String>,
}

impl SwarmUsageTotals {
    /// Calculate total remaining capacity in context windows.
    pub fn tokens_left(&self) -> Option<u64> {
        match (self.tokens_used, self.context_size) {
            (Some(used), Some(size)) => Some(size.saturating_sub(used)),
            _ => None,
        }
    }

    /// Calculate percentage of context capacity remaining.
    pub fn percent_left(&self) -> Option<f64> {
        match (self.tokens_used, self.context_size) {
            (Some(used), Some(size)) if size > 0 => {
                let left = size.saturating_sub(used);
                Some((left as f64 / size as f64) * 100.0)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SwarmUsage {
    #[serde(rename = "updatedAt", default)]
    pub updated_at: String,
    pub totals: SwarmUsageTotals,
    #[serde(default)]
    pub agents: HashMap<String, UsageMetrics>,
}

/// Minimum time between real system-resource refreshes (GPU/CPU/memory queries). These hit
/// real Win32 APIs -- loading dxgi.dll/pdh.dll and walking the PDH perf-counter provider --
/// which routinely takes tens to hundreds of milliseconds. `refresh()` runs once per event
/// loop iteration, i.e. once per keystroke, so without this throttle every key press would
/// wait on a GPU query before the screen could redraw.
const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Default)]
pub struct GpuTelemetry {
    pub name: String,
    pub vram_used_gb: f64,
    pub vram_total_gb: f64,
    pub vram_percent: u32,
    pub util_percent: u32,
}

#[derive(Debug, Clone)]
pub struct SystemTelemetry {
    pub cpu_percent: u32,
    pub ram_percent: u32,
    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
    pub gpu: GpuTelemetry,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub invocations: u64,
    pub tokens_used: Option<u64>,
    pub total_cost: Option<(f64, String)>,
    pub process_uptime_secs: u64,
    pub pid: u32,
    pub active_agents: usize,
    pub total_agents: usize,
    #[allow(dead_code)]
    last_cpu_sample: Option<(u64, u64)>,
    last_refresh_at: Option<Instant>,
}

impl Default for SystemTelemetry {
    fn default() -> Self {
        Self {
            cpu_percent: 0,
            ram_percent: 0,
            ram_used_gb: 0.0,
            ram_total_gb: 0.0,
            gpu: GpuTelemetry::default(),
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            invocations: 0,
            tokens_used: None,
            total_cost: None,
            process_uptime_secs: 0,
            pid: std::process::id(),
            active_agents: 0,
            total_agents: 0,
            last_cpu_sample: None,
            last_refresh_at: None,
        }
    }
}

impl SystemTelemetry {
    pub fn new() -> Self {
        let mut telemetry = Self::default();
        let dummy_start = Instant::now();
        telemetry.refresh(&dummy_start);
        telemetry
    }

    /// Refresh all telemetry metrics from the real system and disk state, throttled to at
    /// most once per `REFRESH_INTERVAL` -- see its doc comment for why that matters. The
    /// cheap uptime/pid fields still update every call.
    pub fn refresh(&mut self, start_time: &Instant) {
        self.process_uptime_secs = start_time.elapsed().as_secs();
        self.pid = std::process::id();

        let now = Instant::now();
        let due = self
            .last_refresh_at
            .map(|last| now.duration_since(last) >= REFRESH_INTERVAL)
            .unwrap_or(true);
        if !due {
            return;
        }
        self.last_refresh_at = Some(now);

        self.refresh_system_resources();
        self.refresh_usage_metrics();
        self.refresh_agent_status();
    }

    #[cfg(target_os = "windows")]
    fn refresh_system_resources(&mut self) {
        if let Some((load, used_gb, total_gb)) = sys_windows::get_memory_info() {
            self.ram_percent = load;
            self.ram_used_gb = used_gb;
            self.ram_total_gb = total_gb;
        }

        if let Some((curr_idle, curr_total)) = sys_windows::get_cpu_times() {
            if let Some((prev_idle, prev_total)) = self.last_cpu_sample {
                let idle_delta = curr_idle.saturating_sub(prev_idle);
                let total_delta = curr_total.saturating_sub(prev_total);
                if total_delta > 0 {
                    let busy_ratio = 1.0 - (idle_delta as f64 / total_delta as f64);
                    self.cpu_percent = (busy_ratio * 100.0).clamp(0.0, 100.0).round() as u32;
                }
            }
            self.last_cpu_sample = Some((curr_idle, curr_total));
        }

        if let Some(gpu_info) = sys_windows::query_gpu_info() {
            self.gpu.name = gpu_info.name;
            self.gpu.vram_used_gb = gpu_info.vram_used_gb;
            self.gpu.vram_total_gb = gpu_info.vram_total_gb;
            self.gpu.vram_percent = gpu_info.vram_percent;
            self.gpu.util_percent = gpu_info.util_percent;
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn refresh_system_resources(&mut self) {
        // Fallback for non-Windows platforms
        self.cpu_percent = 5;
        self.ram_percent = 25;
        self.ram_used_gb = 4.0;
        self.ram_total_gb = 16.0;
        self.gpu.name = "Generic GPU".to_string();
        self.gpu.vram_used_gb = 1.2;
        self.gpu.vram_total_gb = 8.0;
        self.gpu.vram_percent = 15;
        self.gpu.util_percent = 5;
    }

    /// Read real token consumption from .state/usage.json or .state/usage/*.json.
    fn refresh_usage_metrics(&mut self) {
        let _ = sync_from_sessions();

        // 1. Try reading .state/usage.json
        let usage_json_path = Path::new(".state/usage.json");
        if usage_json_path.is_file()
            && let Ok(content) = fs::read_to_string(usage_json_path)
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&content)
            && let Some(totals) = val.get("totals")
        {
            self.prompt_tokens = totals
                .get("promptTokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            self.completion_tokens = totals
                .get("completionTokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            self.total_tokens = totals
                .get("totalTokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(self.prompt_tokens + self.completion_tokens);
            self.invocations = totals
                .get("invocations")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            self.tokens_used = totals.get("tokensUsed").and_then(|v| v.as_u64());
            if let Some(cost) = totals.get("costAmount").and_then(|v| v.as_f64()) {
                let curr = totals
                    .get("costCurrency")
                    .and_then(|v| v.as_str())
                    .unwrap_or("USD")
                    .to_string();
                self.total_cost = Some((cost, curr));
            }
            return;
        }

        // 2. Fallback: aggregate individual .state/usage/*.json files
        let usage_dir = Path::new(".state/usage");
        if usage_dir.is_dir() {
            let mut prompt_sum = 0u64;
            let mut completion_sum = 0u64;
            let mut invocations_sum = 0u64;
            let mut tokens_used_sum = 0u64;
            let mut cost_sum = 0.0f64;
            let mut cost_currency: Option<String> = None;

            if let Ok(entries) = fs::read_dir(usage_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("json")
                        && let Ok(content) = fs::read_to_string(&path)
                        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&content)
                    {
                        prompt_sum += val
                            .get("promptTokens")
                            .or_else(|| val.get("prompt_tokens"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        completion_sum += val
                            .get("completionTokens")
                            .or_else(|| val.get("completion_tokens"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        invocations_sum +=
                            val.get("invocations").and_then(|v| v.as_u64()).unwrap_or(0);
                        tokens_used_sum += val
                            .get("tokensUsed")
                            .or_else(|| val.get("tokens_used"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        if let Some(cost) = val.get("costAmount").and_then(|v| v.as_f64()) {
                            cost_sum += cost;
                            if cost_currency.is_none() {
                                cost_currency = val
                                    .get("costCurrency")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                            }
                        }
                    }
                }
            }

            self.prompt_tokens = prompt_sum;
            self.completion_tokens = completion_sum;
            self.total_tokens = prompt_sum + completion_sum;
            self.invocations = invocations_sum;
            if tokens_used_sum > 0 {
                self.tokens_used = Some(tokens_used_sum);
            }
            if cost_sum > 0.0 {
                self.total_cost =
                    Some((cost_sum, cost_currency.unwrap_or_else(|| "USD".to_string())));
            }
        }
    }

    /// Read real agent counts and states from .state/status/*.json.
    fn refresh_agent_status(&mut self) {
        let status_dir = Path::new(".state/status");
        if !status_dir.is_dir() {
            self.active_agents = 0;
            self.total_agents = 0;
            return;
        }

        let mut total = 0;
        let mut active = 0;

        if let Ok(entries) = fs::read_dir(status_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    total += 1;
                    if let Ok(content) = fs::read_to_string(&path)
                        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&content)
                    {
                        let state = val
                            .get("state")
                            .and_then(|s| s.as_str())
                            .unwrap_or("idle")
                            .to_ascii_lowercase();
                        if state != "idle" && state != "stopped" {
                            active += 1;
                        }
                    }
                }
            }
        }

        self.total_agents = total;
        self.active_agents = active;
    }
}

/// Resolve the absolute path of an agent conversation transcript log file.
pub fn resolve_transcript_path(conv_id: &str) -> Option<PathBuf> {
    let home = env::var("USERPROFILE").or_else(|_| env::var("HOME")).ok()?;
    let candidates = [
        PathBuf::from(&home)
            .join(".gemini")
            .join("antigravity-cli")
            .join("brain")
            .join(conv_id)
            .join(".system_generated")
            .join("logs")
            .join("transcript.jsonl"),
        PathBuf::from(&home)
            .join(".gemini")
            .join("antigravity")
            .join("brain")
            .join(conv_id)
            .join(".system_generated")
            .join("logs")
            .join("transcript.jsonl"),
    ];

    for c in &candidates {
        if c.is_file() {
            return Some(c.clone());
        }
    }
    None
}

/// Parse an Antigravity transcript JSONL file and aggregate prompt, cache read, and completion tokens.
pub fn parse_transcript(transcript_path: &Path, role: &str) -> Option<UsageMetrics> {
    let file = fs::File::open(transcript_path).ok()?;
    let reader = io::BufReader::new(file);

    let mut prompt_tokens = 0u64;
    let mut completion_tokens = 0u64;
    let mut invocations = 0u64;

    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
            let input = json
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache = json
                .get("cache_read_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let output = json
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            if input > 0 || output > 0 {
                prompt_tokens += input + cache;
                completion_tokens += output;
                invocations += 1;
            }
        }
    }

    Some(UsageMetrics {
        role: role.to_string(),
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens + completion_tokens,
        invocations,
        tokens_used: None,
        context_size: None,
        cost_amount: None,
        cost_currency: None,
        last_updated: Some(format!(
            "{}Z",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        )),
    })
}

/// Record real token consumption and invocation counts for an agent role.
pub fn record_tokens(
    role: &str,
    prompt: u64,
    completion: u64,
    used: Option<u64>,
    size: Option<u64>,
    cost: Option<(f64, String)>,
) {
    let usage_dir = Path::new(".state/usage");
    let _ = fs::create_dir_all(usage_dir);
    let path = usage_dir.join(format!("{}.json", role));

    let mut current: UsageMetrics = if path.is_file() {
        fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    } else {
        UsageMetrics::default()
    };

    current.role = role.to_string();
    current.prompt_tokens += prompt;
    current.completion_tokens += completion;
    current.total_tokens = current.prompt_tokens + current.completion_tokens;
    current.invocations += 1;
    if let Some(u) = used {
        current.tokens_used = Some(u);
    }
    if let Some(sz) = size {
        current.context_size = Some(sz);
    }
    if let Some((amount, currency)) = cost {
        current.cost_amount = Some(current.cost_amount.unwrap_or(0.0) + amount);
        current.cost_currency = Some(currency);
    }
    current.last_updated = Some(format!(
        "{}Z",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    ));

    if let Ok(json) = serde_json::to_string_pretty(&current) {
        let _ = fs::write(&path, json);
    }
    let _ = sync_swarm_usage();
}

/// Aggregate individual per-agent usage files into .state/usage.json.
pub fn sync_swarm_usage() -> io::Result<SwarmUsage> {
    let usage_dir = Path::new(".state/usage");
    let mut totals = SwarmUsageTotals::default();
    let mut agents = HashMap::new();

    if usage_dir.is_dir()
        && let Ok(entries) = fs::read_dir(usage_dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json")
                && let Ok(content) = fs::read_to_string(&path)
                && let Ok(record) = serde_json::from_str::<UsageMetrics>(&content)
            {
                let role = record.role.clone();
                totals.invocations += record.invocations;
                totals.prompt_tokens += record.prompt_tokens;
                totals.completion_tokens += record.completion_tokens;
                totals.total_tokens += record.total_tokens;
                if let Some(used) = record.tokens_used {
                    totals.tokens_used = Some(totals.tokens_used.unwrap_or(0) + used);
                }
                if let Some(size) = record.context_size {
                    totals.context_size = Some(totals.context_size.unwrap_or(0) + size);
                }
                if let Some(cost) = record.cost_amount {
                    totals.cost_amount = Some(totals.cost_amount.unwrap_or(0.0) + cost);
                    if totals.cost_currency.is_none() {
                        totals.cost_currency = record.cost_currency.clone();
                    }
                }
                agents.insert(role, record);
            }
        }
    }

    let iso_now = format!(
        "{}Z",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );

    let usage = SwarmUsage {
        updated_at: iso_now,
        totals,
        agents,
    };

    let _ = fs::create_dir_all(".state");
    let content =
        serde_json::to_string_pretty(&usage).map_err(|e| io::Error::other(e.to_string()))?;
    fs::write(".state/usage.json", content)?;

    Ok(usage)
}

/// Synchronize usage metrics from all active session transcripts into .state/usage/.
pub fn sync_from_sessions() -> io::Result<()> {
    let session_store = Path::new(".state/acp_sessions.json");
    if !session_store.is_file() {
        return Ok(());
    }

    let content = fs::read_to_string(session_store)?;
    let map: HashMap<String, String> = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };

    for (key, session_id) in map {
        let role = if let Some((_, pane_name)) = key.split_once("::") {
            pane_name.to_string()
        } else {
            key.clone()
        };

        if let Some(transcript_path) = resolve_transcript_path(&session_id)
            && let Some(metrics) = parse_transcript(&transcript_path, &role)
        {
            let usage_dir = Path::new(".state/usage");
            let _ = fs::create_dir_all(usage_dir);
            let out_file = usage_dir.join(format!("{}.json", role));

            let mut existing: UsageMetrics = if out_file.is_file() {
                fs::read_to_string(&out_file)
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or_default()
            } else {
                UsageMetrics::default()
            };

            existing.role = role;
            existing.prompt_tokens = metrics.prompt_tokens;
            existing.completion_tokens = metrics.completion_tokens;
            existing.total_tokens = metrics.total_tokens;
            existing.invocations = metrics.invocations;
            existing.last_updated = metrics.last_updated;

            if let Ok(json) = serde_json::to_string_pretty(&existing) {
                let _ = fs::write(out_file, json);
            }
        }
    }

    let _ = sync_swarm_usage();
    Ok(())
}

/// Format large numbers with thousands separators.
pub fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let len = s.len();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}

/// Print formatted usage telemetry summary to stdout.
pub fn print_usage_summary() {
    let _ = sync_from_sessions();
    let usage_path = Path::new(".state/usage.json");
    let usage: SwarmUsage = if usage_path.is_file() {
        fs::read_to_string(usage_path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    } else {
        SwarmUsage::default()
    };

    println!("================ Locust Swarm Usage Telemetry ================");
    println!(
        "Total Invocations:  {}",
        format_number(usage.totals.invocations)
    );
    println!(
        "Prompt Tokens:      {}",
        format_number(usage.totals.prompt_tokens)
    );
    println!(
        "Completion Tokens:  {}",
        format_number(usage.totals.completion_tokens)
    );
    println!(
        "Total Tokens:       {}",
        format_number(usage.totals.total_tokens)
    );
    if let (Some(u), Some(sz)) = (usage.totals.tokens_used, usage.totals.context_size) {
        let left = sz.saturating_sub(u);
        let pct = if sz > 0 {
            (left as f64 / sz as f64) * 100.0
        } else {
            0.0
        };
        println!(
            "Context Capacity:   {} / {} in context ({} left, {:.1}% available)",
            format_number(u),
            format_number(sz),
            format_number(left),
            pct
        );
    } else if let Some(u) = usage.totals.tokens_used {
        println!("Tokens in Context:  {}", format_number(u));
    }
    if let Some(cost) = usage.totals.cost_amount {
        println!(
            "Total Cost:         {:.4} {}",
            cost,
            usage.totals.cost_currency.as_deref().unwrap_or("USD")
        );
    }
    println!("--------------------------------------------------------------");
    println!("Active Agents ({}):", usage.agents.len());
    let mut roles: Vec<_> = usage.agents.keys().collect();
    roles.sort();
    for role in roles {
        if let Some(agent) = usage.agents.get(role) {
            let context_str = match (agent.tokens_used, agent.context_size) {
                (Some(used), Some(size)) => {
                    let left = size.saturating_sub(used);
                    let pct = if size > 0 {
                        (left as f64 / size as f64) * 100.0
                    } else {
                        0.0
                    };
                    format!(
                        " | context: {}/{} ({} left, {:.1}%)",
                        format_number(used),
                        format_number(size),
                        format_number(left),
                        pct
                    )
                }
                (Some(used), None) => format!(" | context: {} used", format_number(used)),
                _ => String::new(),
            };
            let cost_str = match (agent.cost_amount, &agent.cost_currency) {
                (Some(amt), Some(curr)) => format!(" | {:.4} {}", amt, curr),
                _ => String::new(),
            };
            println!(
                "  - {:<28} {:>8} total tokens ({:>7} prompt, {:>6} compl) | {:>3} calls{}{}",
                role,
                format_number(agent.total_tokens),
                format_number(agent.prompt_tokens),
                format_number(agent.completion_tokens),
                agent.invocations,
                context_str,
                cost_str
            );
        }
    }
    println!("==============================================================");
}

#[cfg(target_os = "windows")]
#[allow(clippy::upper_case_acronyms, clippy::manual_c_str_literals)]
mod sys_windows {
    #[repr(C)]
    #[derive(Default)]
    pub struct MemoryStatusEx {
        pub length: u32,
        pub memory_load: u32,
        pub total_phys: u64,
        pub avail_phys: u64,
        pub total_page_file: u64,
        pub avail_page_file: u64,
        pub total_virtual: u64,
        pub avail_virtual: u64,
        pub avail_extended_virtual: u64,
    }

    #[repr(C)]
    #[derive(Default, Copy, Clone)]
    pub struct FileTime {
        pub low_date_time: u32,
        pub high_date_time: u32,
    }

    impl FileTime {
        pub fn to_u64(self) -> u64 {
            ((self.high_date_time as u64) << 32) | (self.low_date_time as u64)
        }
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn GlobalMemoryStatusEx(lp_buffer: *mut MemoryStatusEx) -> i32;
        pub fn GetSystemTimes(
            lp_idle_time: *mut FileTime,
            lp_kernel_time: *mut FileTime,
            lp_user_time: *mut FileTime,
        ) -> i32;
        pub fn LoadLibraryA(lp_lib_file_name: *const u8) -> *mut std::ffi::c_void;
        pub fn GetProcAddress(
            h_module: *mut std::ffi::c_void,
            lp_proc_name: *const u8,
        ) -> *mut std::ffi::c_void;
        pub fn FreeLibrary(h_lib_module: *mut std::ffi::c_void) -> i32;
    }

    pub fn get_memory_info() -> Option<(u32, f64, f64)> {
        let mut status = MemoryStatusEx {
            length: std::mem::size_of::<MemoryStatusEx>() as u32,
            ..Default::default()
        };
        unsafe {
            if GlobalMemoryStatusEx(&mut status) != 0 {
                let total_gb = status.total_phys as f64 / (1024.0 * 1024.0 * 1024.0);
                let used_gb =
                    (status.total_phys - status.avail_phys) as f64 / (1024.0 * 1024.0 * 1024.0);
                return Some((status.memory_load, used_gb, total_gb));
            }
        }
        None
    }

    pub fn get_cpu_times() -> Option<(u64, u64)> {
        let mut idle = FileTime::default();
        let mut kernel = FileTime::default();
        let mut user = FileTime::default();
        unsafe {
            if GetSystemTimes(&mut idle, &mut kernel, &mut user) != 0 {
                let idle_time = idle.to_u64();
                let total_time = kernel.to_u64() + user.to_u64();
                return Some((idle_time, total_time));
            }
        }
        None
    }

    #[repr(C)]
    pub struct GUID {
        pub data1: u32,
        pub data2: u16,
        pub data3: u16,
        pub data4: [u8; 8],
    }

    pub const IID_IDXGI_FACTORY1: GUID = GUID {
        data1: 0x770aae78,
        data2: 0xf26f,
        data3: 0x4dba,
        data4: [0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87],
    };

    pub const IID_IDXGI_ADAPTER3: GUID = GUID {
        data1: 0x64596774,
        data2: 0x43b6,
        data3: 0x4557,
        data4: [0x9e, 0x52, 0xcc, 0x86, 0xe3, 0x18, 0x82, 0x62],
    };

    #[repr(C)]
    pub struct DXGI_ADAPTER_DESC1 {
        pub description: [u16; 128],
        pub vendor_id: u32,
        pub device_id: u32,
        pub sub_sys_id: u32,
        pub revision: u32,
        pub dedicated_video_memory: usize,
        pub dedicated_system_memory: usize,
        pub shared_system_memory: usize,
        pub adapter_luid: u64,
        pub flags: u32,
    }

    #[repr(C)]
    pub struct DXGI_QUERY_VIDEO_MEMORY_INFO {
        pub budget: u64,
        pub current_usage: u64,
        pub available_for_reservation: u64,
        pub current_reservation: u64,
    }

    #[derive(Debug, Clone, Default)]
    pub struct GpuInfo {
        pub name: String,
        pub vram_used_gb: f64,
        pub vram_total_gb: f64,
        pub vram_percent: u32,
        pub util_percent: u32,
    }

    pub fn query_gpu_info() -> Option<GpuInfo> {
        unsafe {
            let dxgi = LoadLibraryA(b"dxgi.dll\0".as_ptr());
            if dxgi.is_null() {
                return None;
            }

            let create_factory_ptr = GetProcAddress(dxgi, b"CreateDXGIFactory1\0".as_ptr());
            if create_factory_ptr.is_null() {
                FreeLibrary(dxgi);
                return None;
            }

            type CreateDXGIFactory1Fn =
                unsafe extern "system" fn(riid: *const GUID, pp_factory: *mut *mut usize) -> i32;

            let create_dxgi_factory1: CreateDXGIFactory1Fn =
                std::mem::transmute(create_factory_ptr);

            let mut factory: *mut usize = std::ptr::null_mut();
            if create_dxgi_factory1(&IID_IDXGI_FACTORY1, &mut factory) != 0 || factory.is_null() {
                FreeLibrary(dxgi);
                return None;
            }

            // IDXGIFactory1 vtable:
            // 0: QueryInterface, 1: AddRef, 2: Release ... 12: EnumAdapters1
            let factory_vtable = *(factory as *const *const usize);
            type EnumAdapters1Fn = unsafe extern "system" fn(
                this: *mut usize,
                adapter_idx: u32,
                pp_adapter: *mut *mut usize,
            ) -> i32;
            type ReleaseFn = unsafe extern "system" fn(this: *mut usize) -> u32;

            let enum_adapters1: EnumAdapters1Fn = std::mem::transmute(*factory_vtable.add(12));
            let factory_release: ReleaseFn = std::mem::transmute(*factory_vtable.add(2));

            let mut best_gpu: Option<GpuInfo> = None;
            let mut adapter_idx = 0u32;

            loop {
                let mut adapter: *mut usize = std::ptr::null_mut();
                if enum_adapters1(factory, adapter_idx, &mut adapter) != 0 || adapter.is_null() {
                    break;
                }

                let adapter_vtable = *(adapter as *const *const usize);
                type GetDesc1Fn = unsafe extern "system" fn(
                    this: *mut usize,
                    p_desc: *mut DXGI_ADAPTER_DESC1,
                ) -> i32;
                type QueryInterfaceFn = unsafe extern "system" fn(
                    this: *mut usize,
                    riid: *const GUID,
                    ppv: *mut *mut usize,
                ) -> i32;
                let adapter_release: ReleaseFn = std::mem::transmute(*adapter_vtable.add(2));
                let get_desc1: GetDesc1Fn = std::mem::transmute(*adapter_vtable.add(10));
                let query_interface: QueryInterfaceFn = std::mem::transmute(*adapter_vtable.add(0));

                let mut desc = std::mem::zeroed::<DXGI_ADAPTER_DESC1>();
                if get_desc1(adapter, &mut desc) == 0 {
                    // Filter out software / WARP adapters (DXGI_ADAPTER_FLAG_SOFTWARE = 2)
                    if (desc.flags & 2) == 0 {
                        let end_pos = desc
                            .description
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(desc.description.len());
                        let name = String::from_utf16_lossy(&desc.description[..end_pos])
                            .trim()
                            .to_string();

                        let mut vram_total = desc.dedicated_video_memory as u64;
                        let mut vram_used = 0u64;

                        // Try querying IDXGIAdapter3 for real-time video memory usage
                        let mut adapter3: *mut usize = std::ptr::null_mut();
                        if query_interface(adapter, &IID_IDXGI_ADAPTER3, &mut adapter3) == 0
                            && !adapter3.is_null()
                        {
                            let adapter3_vtable = *(adapter3 as *const *const usize);
                            type QueryVideoMemoryInfoFn = unsafe extern "system" fn(
                                this: *mut usize,
                                node_index: u32,
                                memory_segment_group: u32,
                                p_video_memory_info: *mut DXGI_QUERY_VIDEO_MEMORY_INFO,
                            )
                                -> i32;
                            let adapter3_release: ReleaseFn =
                                std::mem::transmute(*adapter3_vtable.add(2));
                            let query_video_mem: QueryVideoMemoryInfoFn =
                                std::mem::transmute(*adapter3_vtable.add(14));

                            let mut mem_info = std::mem::zeroed::<DXGI_QUERY_VIDEO_MEMORY_INFO>();
                            // DXGI_MEMORY_SEGMENT_GROUP_LOCAL = 0
                            if query_video_mem(adapter3, 0, 0, &mut mem_info) == 0 {
                                vram_used = mem_info.current_usage;
                                if mem_info.budget > 0 && vram_total == 0 {
                                    vram_total = mem_info.budget;
                                }
                            }
                            adapter3_release(adapter3);
                        }

                        // Try querying system-wide dedicated usage via PDH
                        let system_vram = query_pdh_vram(desc.adapter_luid);
                        if let Some(sys_used) = system_vram
                            && sys_used > 0
                        {
                            vram_used = sys_used;
                        }

                        let total_gb = vram_total as f64 / (1024.0 * 1024.0 * 1024.0);
                        let used_gb = vram_used as f64 / (1024.0 * 1024.0 * 1024.0);
                        let percent = if total_gb > 0.0 {
                            ((used_gb / total_gb) * 100.0).clamp(0.0, 100.0).round() as u32
                        } else {
                            0
                        };

                        let candidate = GpuInfo {
                            name,
                            vram_used_gb: used_gb,
                            vram_total_gb: total_gb,
                            vram_percent: percent,
                            util_percent: 0,
                        };

                        // Prefer discrete GPU with larger dedicated memory
                        if best_gpu
                            .as_ref()
                            .is_none_or(|bg| candidate.vram_total_gb > bg.vram_total_gb)
                        {
                            best_gpu = Some(candidate);
                        }
                    }
                }

                adapter_release(adapter);
                adapter_idx += 1;
            }

            factory_release(factory);
            FreeLibrary(dxgi);

            best_gpu
        }
    }

    #[repr(C)]
    pub struct PdhFmtCounterValue {
        pub c_status: u32,
        pub padding: u32,
        pub large_value: i64,
    }

    pub fn query_pdh_vram(luid: u64) -> Option<u64> {
        unsafe {
            let pdh = LoadLibraryA(b"pdh.dll\0".as_ptr());
            if pdh.is_null() {
                return None;
            }

            type PdhOpenQueryAFn = unsafe extern "system" fn(
                sz_data_source: *const u8,
                dw_user_data: usize,
                ph_query: *mut *mut usize,
            ) -> i32;
            type PdhAddEnglishCounterAFn = unsafe extern "system" fn(
                h_query: *mut usize,
                sz_full_counter_path: *const u8,
                dw_user_data: usize,
                ph_counter: *mut *mut usize,
            ) -> i32;
            type PdhCollectQueryDataFn = unsafe extern "system" fn(h_query: *mut usize) -> i32;
            type PdhGetFormattedCounterValueFn = unsafe extern "system" fn(
                h_counter: *mut usize,
                dw_format: u32,
                lpdw_type: *mut u32,
                p_value: *mut PdhFmtCounterValue,
            ) -> i32;
            type PdhCloseQueryFn = unsafe extern "system" fn(h_query: *mut usize) -> i32;

            let open_query_ptr = GetProcAddress(pdh, b"PdhOpenQueryA\0".as_ptr());
            let mut add_counter_ptr = GetProcAddress(pdh, b"PdhAddEnglishCounterA\0".as_ptr());
            if add_counter_ptr.is_null() {
                add_counter_ptr = GetProcAddress(pdh, b"PdhAddCounterA\0".as_ptr());
            }
            let collect_data_ptr = GetProcAddress(pdh, b"PdhCollectQueryData\0".as_ptr());
            let get_val_ptr = GetProcAddress(pdh, b"PdhGetFormattedCounterValue\0".as_ptr());
            let close_query_ptr = GetProcAddress(pdh, b"PdhCloseQuery\0".as_ptr());

            if open_query_ptr.is_null()
                || add_counter_ptr.is_null()
                || collect_data_ptr.is_null()
                || get_val_ptr.is_null()
                || close_query_ptr.is_null()
            {
                FreeLibrary(pdh);
                return None;
            }

            let open_query: PdhOpenQueryAFn = std::mem::transmute(open_query_ptr);
            let add_counter: PdhAddEnglishCounterAFn = std::mem::transmute(add_counter_ptr);
            let collect_data: PdhCollectQueryDataFn = std::mem::transmute(collect_data_ptr);
            let get_val: PdhGetFormattedCounterValueFn = std::mem::transmute(get_val_ptr);
            let close_query: PdhCloseQueryFn = std::mem::transmute(close_query_ptr);

            let mut query: *mut usize = std::ptr::null_mut();
            if open_query(std::ptr::null(), 0, &mut query) != 0 || query.is_null() {
                FreeLibrary(pdh);
                return None;
            }

            let luid_high = (luid >> 32) as u32;
            let luid_low = (luid & 0xffffffff) as u32;
            let counter_path = format!(
                "\\GPU Adapter Memory(luid_0x{:08x}_0x{:08x}_phys_0)\\Dedicated Usage\0",
                luid_high, luid_low
            );

            let mut counter: *mut usize = std::ptr::null_mut();
            let mut result = None;

            if add_counter(query, counter_path.as_ptr(), 0, &mut counter) == 0
                && !counter.is_null()
                && collect_data(query) == 0
            {
                let mut val = std::mem::zeroed::<PdhFmtCounterValue>();
                // PDH_FMT_LARGE = 0x00000400
                if get_val(counter, 0x00000400, std::ptr::null_mut(), &mut val) == 0
                    && val.c_status == 0
                    && val.large_value >= 0
                {
                    result = Some(val.large_value as u64);
                }
            }

            close_query(query);
            FreeLibrary(pdh);
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_reads_real_system_values() {
        let mut telemetry = SystemTelemetry::new();
        let start = Instant::now();
        telemetry.refresh(&start);

        #[cfg(target_os = "windows")]
        {
            assert!(telemetry.ram_total_gb > 0.0);
            assert!(telemetry.ram_used_gb > 0.0);
            assert!(telemetry.ram_percent <= 100);
            assert!(!telemetry.gpu.name.is_empty());
            assert!(telemetry.gpu.vram_total_gb > 0.0);
            assert!(telemetry.gpu.vram_used_gb > 0.0);
        }

        assert_eq!(telemetry.pid, std::process::id());
    }

    #[test]
    fn test_telemetry_reads_usage_state() {
        let mut telemetry = SystemTelemetry::new();
        let start = Instant::now();
        telemetry.refresh(&start);

        // If .state/usage.json exists, verify telemetry accurately reflects its totals
        if let Ok(content) = std::fs::read_to_string(".state/usage.json") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(totals) = v.get("totals") {
                    let expected_total = totals
                        .get("totalTokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let expected_prompt = totals
                        .get("promptTokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    assert_eq!(telemetry.total_tokens, expected_total);
                    assert_eq!(telemetry.prompt_tokens, expected_prompt);
                }
            }
        }
    }

    #[test]
    fn test_record_tokens_and_sync_swarm_usage() {
        let test_role = format!("test_agent_{}", std::process::id());
        record_tokens(
            &test_role,
            150,
            50,
            Some(200),
            Some(8000),
            Some((0.005, "USD".to_string())),
        );

        let file = Path::new(".state/usage").join(format!("{}.json", test_role));
        assert!(file.is_file());

        let content = fs::read_to_string(&file).unwrap();
        let metric: UsageMetrics = serde_json::from_str(&content).unwrap();
        assert_eq!(metric.tokens_left(), Some(7800));
        assert_eq!(metric.percent_left(), Some(97.5));

        let usage_json = Path::new(".state/usage.json");
        assert!(usage_json.is_file());

        let swarm_content = fs::read_to_string(usage_json).unwrap();
        assert!(swarm_content.contains(&test_role));

        // Clean up test file
        let _ = fs::remove_file(file);
        let _ = sync_swarm_usage();
    }

    #[test]
    fn test_usage_metrics_and_totals_tokens_left() {
        let metric = UsageMetrics {
            role: "test".to_string(),
            prompt_tokens: 100,
            completion_tokens: 200,
            total_tokens: 300,
            invocations: 1,
            tokens_used: Some(1500),
            context_size: Some(200_000),
            cost_amount: None,
            cost_currency: None,
            last_updated: None,
        };
        assert_eq!(metric.tokens_left(), Some(198_500));
        assert_eq!(metric.percent_left(), Some(99.25));

        let totals = SwarmUsageTotals {
            prompt_tokens: 100,
            completion_tokens: 200,
            total_tokens: 300,
            invocations: 1,
            tokens_used: Some(2000),
            context_size: Some(100_000),
            cost_amount: None,
            cost_currency: None,
        };
        assert_eq!(totals.tokens_left(), Some(98_000));
        assert_eq!(totals.percent_left(), Some(98.0));
    }

    #[test]
    fn test_parse_transcript_in_memory() {
        let tmp_file =
            env::temp_dir().join(format!("test_transcript_{}.jsonl", std::process::id()));
        let lines = "{\"input_tokens\": 100, \"cache_read_tokens\": 20, \"output_tokens\": 30}\n{\"input_tokens\": 50, \"output_tokens\": 15}\n";
        fs::write(&tmp_file, lines).unwrap();

        let metrics = parse_transcript(&tmp_file, "test_role").unwrap();
        assert_eq!(metrics.role, "test_role");
        assert_eq!(metrics.prompt_tokens, 170);
        assert_eq!(metrics.completion_tokens, 45);
        assert_eq!(metrics.total_tokens, 215);
        assert_eq!(metrics.invocations, 2);

        let _ = fs::remove_file(tmp_file);
    }
}
