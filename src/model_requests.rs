//! Bookkeeping for model-list requests: skips refetching for unchanged endpoint settings and
//! identifies the latest request, so only its result is shown.

/// Identifies one model-list request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket(u64);

#[derive(Debug, Default)]
pub struct ModelRequests {
    generation: u64,
    last_requested: Option<(String, String)>,
}

impl ModelRequests {
    /// Starts a request for `(base_url, api_key)`. Returns `None` if the same pair was requested
    /// last and `force` is false.
    pub fn begin(&mut self, base_url: &str, api_key: &str, force: bool) -> Option<Ticket> {
        let pair = (base_url.to_string(), api_key.to_string());
        if !force && self.last_requested.as_ref() == Some(&pair) {
            return None;
        }
        self.last_requested = Some(pair);
        self.generation += 1;
        Some(Ticket(self.generation))
    }

    /// True if `ticket` belongs to the most recent request.
    pub fn is_current(&self, ticket: Ticket) -> bool {
        ticket.0 == self.generation
    }
}

/// Status line for a successfully loaded model list.
pub fn summary(count: usize) -> String {
    match count {
        0 => "The endpoint lists no models.".to_string(),
        1 => "1 model available.".to_string(),
        n => format!("{n} models available."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_request_always_starts() {
        let mut requests = ModelRequests::default();
        assert!(requests.begin("http://a/v1", "", false).is_some());
    }

    #[test]
    fn unchanged_settings_do_not_refetch_unless_forced() {
        let mut requests = ModelRequests::default();
        requests.begin("http://a/v1", "key", false).unwrap();
        assert_eq!(requests.begin("http://a/v1", "key", false), None);
        assert!(requests.begin("http://a/v1", "key", true).is_some());
    }

    #[test]
    fn changed_url_or_key_refetches() {
        let mut requests = ModelRequests::default();
        requests.begin("http://a/v1", "key", false).unwrap();
        assert!(requests.begin("http://b/v1", "key", false).is_some());
        assert!(requests.begin("http://b/v1", "other", false).is_some());
    }

    #[test]
    fn only_the_latest_ticket_is_current() {
        let mut requests = ModelRequests::default();
        let first = requests.begin("http://a/v1", "", false).unwrap();
        assert!(requests.is_current(first));
        let second = requests.begin("http://a/v1", "", true).unwrap();
        assert!(!requests.is_current(first));
        assert!(requests.is_current(second));
    }

    #[test]
    fn summary_counts_models() {
        assert_eq!(summary(0), "The endpoint lists no models.");
        assert_eq!(summary(1), "1 model available.");
        assert_eq!(summary(3), "3 models available.");
    }
}
