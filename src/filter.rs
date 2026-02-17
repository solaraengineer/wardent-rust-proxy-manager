use hyper::{Response, StatusCode};
use http_body_util::Full;
use bytes::Bytes;
use regex::RegexSet;
use tracing::warn;

use crate::config::FilterConfig;

pub struct Filter {
    blocked_agents: RegexSet,
    redirect_url: String,
}

impl Filter {
    pub fn new(config: &FilterConfig) -> Self {
        // Build case-insensitive regex patterns from blocked user-agent strings
        let patterns: Vec<String> = config
            .blocked_user_agents
            .iter()
            .map(|ua| format!("(?i){}", regex::escape(ua)))
            .collect();

        let blocked_agents = RegexSet::new(&patterns)
            .expect("Failed to compile user-agent regex patterns");

        Self {
            blocked_agents,
            redirect_url: config.redirect_url.clone(),
        }
    }

    /// Check if a user-agent string matches any blocked pattern.
    /// Returns Some(Response) with 301 redirect if blocked, None if allowed.
    pub fn check_user_agent(
        &self,
        user_agent: Option<&str>,
    ) -> Option<Response<Full<Bytes>>> {
        let ua = match user_agent {
            Some(ua) => ua,
            None => return None, // No UA header = let through
        };

        if self.blocked_agents.is_match(ua) {
            warn!(user_agent = ua, "Blocked bot user-agent, redirecting");

            let response = Response::builder()
                .status(StatusCode::MOVED_PERMANENTLY)
                .header("Location", &self.redirect_url)
                .header("Content-Length", "0")
                .body(Full::new(Bytes::new()))
                .unwrap();

            return Some(response);
        }

        None
    }
}
