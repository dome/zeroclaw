use super::traits::{Channel, ChannelMessage, SendMessage};
use anyhow::Result;
use async_trait::async_trait;
use regex::Regex;
use uuid::Uuid;

/// Facebook Messenger channel — uses Meta Graph API for Facebook Pages
///
/// This channel operates in webhook mode (push-based) rather than polling.
/// Messages are received via the gateway's `/facebook-webhook` webhook endpoint.
/// The `listen` method here is a no-op placeholder; actual message handling
/// happens in the gateway when Meta sends webhook events.
fn ensure_https(url: &str) -> anyhow::Result<()> {
    if !url.starts_with("https://") {
        anyhow::bail!(
            "Refusing to transmit sensitive data over non-HTTPS URL: URL scheme must be https"
        );
    }
    Ok(())
}

pub struct FacebookChannel {
    access_token: String,
    page_id: String,
    verify_token: String,
    allowed_senders: Vec<String>,
    /// Per-channel proxy URL override.
    proxy_url: Option<String>,
    /// Compiled mention patterns for DM mention gating.
    dm_mention_patterns: Vec<Regex>,
    /// Compiled mention patterns for group-chat mention gating.
    group_mention_patterns: Vec<Regex>,
}

impl FacebookChannel {
    pub fn new(
        access_token: String,
        page_id: String,
        verify_token: String,
        allowed_senders: Vec<String>,
    ) -> Self {
        Self {
            access_token,
            page_id,
            verify_token,
            allowed_senders,
            proxy_url: None,
            dm_mention_patterns: Vec::new(),
            group_mention_patterns: Vec::new(),
        }
    }

    /// Set a per-channel proxy URL that overrides the global proxy config.
    pub fn with_proxy_url(mut self, proxy_url: Option<String>) -> Self {
        self.proxy_url = proxy_url;
        self
    }

    /// Set mention patterns for DM mention gating.
    /// Each pattern string is compiled as a case-insensitive regex.
    /// Invalid patterns are logged and skipped.
    pub fn with_dm_mention_patterns(mut self, patterns: Vec<String>) -> Self {
        self.dm_mention_patterns = Self::compile_mention_patterns(&patterns);
        self
    }

    /// Set mention patterns for group-chat mention gating.
    /// Each pattern string is compiled as a case-insensitive regex.
    /// Invalid patterns are logged and skipped.
    pub fn with_group_mention_patterns(mut self, patterns: Vec<String>) -> Self {
        self.group_mention_patterns = Self::compile_mention_patterns(&patterns);
        self
    }

    /// Compile raw pattern strings into case-insensitive regexes.
    /// Invalid or excessively large patterns are logged and skipped.
    pub(crate) fn compile_mention_patterns(patterns: &[String]) -> Vec<Regex> {
        patterns
            .iter()
            .filter_map(|p| {
                let trimmed = p.trim();
                if trimmed.is_empty() {
                    return None;
                }
                match regex::RegexBuilder::new(trimmed)
                    .case_insensitive(true)
                    .size_limit(1 << 16) // 64 KiB — guard against ReDoS
                    .build()
                {
                    Ok(re) => Some(re),
                    Err(e) => {
                        tracing::warn!(
                            "Facebook: ignoring invalid mention_pattern {trimmed:?}: {e}"
                        );
                        None
                    }
                }
            })
            .collect()
    }

    /// Check whether `text` matches any pattern in the given slice.
    pub(crate) fn text_matches_patterns(patterns: &[Regex], text: &str) -> bool {
        patterns.iter().any(|re| re.is_match(text))
    }

    /// Strip all pattern matches from `text`, collapse whitespace,
    /// and return `None` if the result is empty.
    pub(crate) fn strip_patterns(patterns: &[Regex], text: &str) -> Option<String> {
        let mut result = text.to_string();
        for re in patterns {
            result = re.replace_all(&result, " ").into_owned();
        }
        let normalized = result.split_whitespace().collect::<Vec<_>>().join(" ");
        (!normalized.is_empty()).then_some(normalized)
    }

    /// Apply mention-pattern gating for a message.
    ///
    /// Selects the appropriate pattern set based on `is_group` and applies
    /// mention gating: when patterns are non-empty, messages that do not
    /// match any pattern are dropped (`None`); messages that match have
    /// the matched fragments stripped.
    /// When the applicable pattern set is empty the original content is
    /// returned unchanged.
    pub(crate) fn apply_mention_gating(
        dm_patterns: &[Regex],
        group_patterns: &[Regex],
        content: &str,
        is_group: bool,
    ) -> Option<String> {
        let patterns = if is_group {
            group_patterns
        } else {
            dm_patterns
        };
        if patterns.is_empty() {
            return Some(content.to_string());
        }
        if !Self::text_matches_patterns(patterns, content) {
            return None;
        }
        Self::strip_patterns(patterns, content)
    }

    /// Detect group messages in the Facebook Graph API webhook payload.
    fn is_group_message(msg: &serde_json::Value) -> bool {
        msg.get("thread_type")
            .and_then(|t| t.as_str())
            .is_some_and(|s| s == "group")
    }

    fn http_client(&self) -> reqwest::Client {
        crate::config::build_channel_proxy_client("channel.facebook", self.proxy_url.as_deref())
    }

    /// Check if a sender PSID is allowed
    fn is_sender_allowed(&self, psid: &str) -> bool {
        self.allowed_senders.iter().any(|n| n == "*" || n == psid)
    }

    /// Get the verify token for webhook verification
    pub fn verify_token(&self) -> &str {
        &self.verify_token
    }

    /// Parse an incoming webhook payload from Meta and extract messages
    pub fn parse_webhook_payload(&self, payload: &serde_json::Value) -> Vec<ChannelMessage> {
        let mut messages = Vec::new();

        // Facebook Page webhook structure:
        // { "object": "page", "entry": [...] }
        let Some(entries) = payload.get("entry").and_then(|e| e.as_array()) else {
            return messages;
        };

        for entry in entries {
            let Some(messaging) = entry.get("messaging").and_then(|m| m.as_array()) else {
                continue;
            };

            for event in messaging {
                let Some(sender) = event.get("sender").and_then(|s| s.get("id")).and_then(|id| id.as_str()) else {
                    continue;
                };

                if !self.is_sender_allowed(sender) {
                    tracing::warn!("Facebook: ignoring message from unauthorized sender: {sender}");
                    continue;
                }

                // Handle text messages
                if let Some(msg) = event.get("message") {
                    if let Some(text) = msg.get("text").and_then(|t| t.as_str()) {
                        let is_group = Self::is_group_message(msg);

                        let content = match Self::apply_mention_gating(
                            &self.dm_mention_patterns,
                            &self.group_mention_patterns,
                            text,
                            is_group
                        ) {
                            Some(c) => c,
                            None => text.to_string(),
                        };

                        let timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();

                        messages.push(ChannelMessage {
                            id: Uuid::new_v4().to_string(),
                            sender: sender.to_string(),
                            reply_target: sender.to_string(),
                            content,
                            channel: "facebook".to_string(),
                            timestamp,
                            thread_ts: Some(sender.to_string()),
                            interruption_scope_id: None,
                            attachments: vec![],
                        });
                    }
                }

                // Handle postback buttons
                if let Some(postback) = event.get("postback") {
                    if let Some(payload) = postback.get("payload").and_then(|p| p.as_str()) {
                        let timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();

                        messages.push(ChannelMessage {
                            id: Uuid::new_v4().to_string(),
                            sender: sender.to_string(),
                            reply_target: sender.to_string(),
                            content: payload.to_string(),
                            channel: "facebook".to_string(),
                            timestamp,
                            thread_ts: Some(sender.to_string()),
                            interruption_scope_id: None,
                            attachments: vec![],
                        });
                    }
                }
            }
        }

        messages
    }
}

#[async_trait]
impl Channel for FacebookChannel {
    fn name(&self) -> &str {
        "facebook"
    }

    async fn send(&self, message: &SendMessage) -> Result<()> {
        tracing::info!("📤 Sending reply to Facebook PSID: {}", message.recipient);
        tracing::debug!("📤 Reply content: {}", message.content);

        let client = self.http_client();

        let payload = serde_json::json!({
            "recipient": {
                "id": message.recipient
            },
            "message": {
                "text": message.content
            },
            "messaging_type": "RESPONSE"
        });

        let resp = client
            .post(format!("https://graph.facebook.com/v21.0/{}/messages", self.page_id))
            .header("Authorization", format!("Bearer {}", self.access_token))
            .json(&payload)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response: {e}>"));
            tracing::error!("❌ Facebook send failed ({status}): {body}");
            anyhow::bail!("Facebook send failed ({status}): {body}");
        }

        tracing::info!("✅ Facebook reply sent successfully to PSID: {}", message.recipient);
        Ok(())
    }

    async fn listen(&self, _tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<()> {
        // Facebook channel uses gateway webhook endpoint for inbound messages
        // This method is a no-op, actual listening happens in gateway HTTP server
        Ok(())
    }

    async fn health_check(&self) -> bool {
        true
    }
}
