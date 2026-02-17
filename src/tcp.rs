use std::time::Duration;
use crate::config::Config;

pub struct TcpConfig<'a> {
    config: &'a Config,
}

impl<'a> TcpConfig<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }

    /// Get the timeout duration for a given request path.
    pub fn timeout_for_path(&self, path: &str) -> Duration {
        let secs = self.config.timeout_for_path(path);
        Duration::from_secs(secs)
    }

    /// Get the default timeout duration.
    pub fn default_timeout(&self) -> Duration {
        Duration::from_secs(self.config.limits.default_timeout_secs)
    }
}
