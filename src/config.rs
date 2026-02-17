use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub proxy: ProxyConfig,
    pub limits: LimitsConfig,
    pub rate_limit: RateLimitConfig,
    pub filter: FilterConfig,
    pub error_redirects: ErrorRedirects,
    #[serde(default)]
    pub timeout_override: Vec<TimeoutOverride>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub listen_addr: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProxyConfig {
    pub upstream: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LimitsConfig {
    pub max_body_size: u64,
    pub default_timeout_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub burst_size: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FilterConfig {
    pub blocked_user_agents: Vec<String>,
    pub redirect_url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TimeoutOverride {
    pub path: String,
    pub timeout_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ErrorRedirects {
    pub rate_limited: String,
    pub banned: String,
    pub body_too_large: String,
    pub timeout: String,
    pub bad_gateway: String,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Get the timeout for a given request path.
    /// Checks timeout_override rules in order, returns first match.
    /// Falls back to default_timeout_secs.
    pub fn timeout_for_path(&self, path: &str) -> u64 {
        for rule in &self.timeout_override {
            if path.starts_with(&rule.path) {
                return rule.timeout_secs;
            }
        }
        self.limits.default_timeout_secs
    }
}
