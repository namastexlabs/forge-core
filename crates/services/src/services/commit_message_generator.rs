use std::path::Path;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommitMessageError {
    #[error("Git service error: {0}")]
    GitError(String),

    #[error("Invalid commit message format")]
    InvalidFormat,
}

/// Service for generating high-quality conventional commit messages
pub struct CommitMessageGenerator;

impl CommitMessageGenerator {
    pub fn new() -> Self {
        Self
    }

    /// Generate a commit message from task context
    ///
    /// Priority order:
    /// 1. Use executor-generated commit message (if available)
    /// 2. Generate from diff analysis
    /// 3. Fallback to sanitized task title
    pub fn generate(
        &self,
        task_title: &str,
        task_description: Option<&str>,
        github_issue: Option<u32>,
        executor_commit_message: Option<&str>,
        _worktree_path: &Path,
    ) -> Result<String, CommitMessageError> {
        // Priority 1: Use executor-generated commit message
        if let Some(msg) = executor_commit_message {
            if Self::is_valid_commit_message(msg) {
                return Ok(msg.to_string());
            }
        }

        // Priority 2: TODO - Analyze diff and generate (future enhancement)
        // This would call commit-suggester agent or use a lightweight model

        // Priority 3: Sanitize task title and construct message
        Ok(Self::sanitize_and_format(
            task_title,
            task_description,
            github_issue,
        ))
    }

    /// Sanitize task title and format as conventional commit
    fn sanitize_and_format(
        title: &str,
        description: Option<&str>,
        github_issue: Option<u32>,
    ) -> String {
        // Remove conversational AI prefixes and clean up
        let cleaned = Self::sanitize_title(title);

        // Build commit message
        let mut message = cleaned;

        // Add GitHub issue reference if available
        if let Some(issue) = github_issue {
            message = format!("{} (#{issue})", message);
        }

        // Add description if it exists and is reasonable
        if let Some(desc) = description {
            let sanitized_desc = Self::sanitize_description(desc);
            if !sanitized_desc.is_empty() && sanitized_desc.len() < 500 {
                message.push_str("\n\n");
                message.push_str(&sanitized_desc);
            }
        }

        message
    }

    /// Sanitize task title - remove conversational crud
    fn sanitize_title(raw_title: &str) -> String {
        let conversational_prefixes = [
            "Perfect! Let me ",
            "Perfect! ",
            "Good, I can see ",
            "Good, I ",
            "Good, ",
            "Let me ",
            "I'll ",
            "I will ",
            "I can ",
            "Sure, I'll ",
            "Sure, ",
            "Okay, I'll ",
            "Okay, ",
            "Great! I'll ",
            "Great! ",
        ];

        let mut cleaned = raw_title.trim();

        // Remove conversational prefixes
        for prefix in &conversational_prefixes {
            if let Some(stripped) = cleaned.strip_prefix(prefix) {
                cleaned = stripped;
                break;
            }
        }

        // Take only first line (summary)
        cleaned = cleaned.lines().next().unwrap_or(cleaned);

        // Truncate to reasonable length (72 chars for subject line)
        // Use chars() to avoid UTF-8 boundary panic on multi-byte characters
        let cleaned: String = cleaned.chars().take(72).collect();

        // Remove trailing ellipsis or incomplete sentences
        let cleaned = cleaned.trim_end_matches("…").trim_end_matches("...");

        cleaned.trim().to_string()
    }

    /// Sanitize description - remove markdown tables, emojis, excessive formatting
    fn sanitize_description(raw_desc: &str) -> String {
        let lines: Vec<&str> = raw_desc
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                // Skip markdown tables
                !trimmed.starts_with('|')
                    && !trimmed.starts_with("---")
                    // Skip section headers with excessive formatting
                    && !trimmed.starts_with("## ")
                    && !trimmed.starts_with("### ")
            })
            .skip_while(|line| line.trim().is_empty()) // Skip leading empty lines
            .take(10) // Max 10 lines
            .collect();

        lines.join("\n").trim().to_string()
    }

    /// Validate commit message format
    fn is_valid_commit_message(msg: &str) -> bool {
        if msg.is_empty() {
            return false;
        }

        // Check for conversational patterns
        let conversational_patterns = [
            "Perfect!",
            "Good, I",
            "Let me",
            "I'll",
            "I will",
            "I can see",
            "Sure,",
            "Okay,",
        ];

        let first_line = msg.lines().next().unwrap_or("");

        // Reject if starts with conversational pattern
        for pattern in &conversational_patterns {
            if first_line.starts_with(pattern) {
                return false;
            }
        }

        // Basic sanity checks
        first_line.len() > 5 && first_line.len() < 200
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_title_removes_conversational_prefixes() {
        assert_eq!(
            CommitMessageGenerator::sanitize_title(
                "Perfect! Let me create a summary for you:"
            ),
            "create a summary for you:"
        );

        assert_eq!(
            CommitMessageGenerator::sanitize_title(
                "Good, I can see the pattern. Now let me create the complete…"
            ),
            "the pattern. Now let me create the complete"
        );

        assert_eq!(
            CommitMessageGenerator::sanitize_title("Let me implement the feature"),
            "implement the feature"
        );
    }

    #[test]
    fn test_sanitize_title_takes_first_line() {
        assert_eq!(
            CommitMessageGenerator::sanitize_title("First line\nSecond line\nThird line"),
            "First line"
        );
    }

    #[test]
    fn test_sanitize_title_truncates_long_lines() {
        let long_title = "a".repeat(100);
        let result = CommitMessageGenerator::sanitize_title(&long_title);
        // 72 chars, not 72 bytes
        assert_eq!(result.chars().count(), 72);
    }

    #[test]
    fn test_sanitize_title_handles_emoji_truncation() {
        // Emoji are 4 bytes each, this tests UTF-8 safe truncation
        let title_with_emoji = format!("{}🚀🎉✨", "a".repeat(70));
        let result = CommitMessageGenerator::sanitize_title(&title_with_emoji);
        // Should truncate to 72 chars without panicking
        assert_eq!(result.chars().count(), 72);
        assert!(result.ends_with("🚀🎉")); // 70 a's + 2 emoji = 72 chars
    }

    #[test]
    fn test_sanitize_title_handles_cjk_characters() {
        // CJK characters are 3 bytes each
        let cjk_title = "这是一个很长的中文标题需要被截断到七十二个字符以内测试多字节字符处理";
        let result = CommitMessageGenerator::sanitize_title(cjk_title);
        // Should not panic and should truncate by char count
        assert!(result.chars().count() <= 72);
    }

    #[test]
    fn test_is_valid_commit_message() {
        assert!(CommitMessageGenerator::is_valid_commit_message(
            "feat: add new feature"
        ));

        assert!(!CommitMessageGenerator::is_valid_commit_message(
            "Perfect! Let me help you"
        ));

        assert!(!CommitMessageGenerator::is_valid_commit_message(""));

        assert!(!CommitMessageGenerator::is_valid_commit_message("abc")); // Too short
    }

    #[test]
    fn test_sanitize_and_format_with_issue() {
        let result = CommitMessageGenerator::sanitize_and_format(
            "implement OAuth login",
            None,
            Some(123),
        );

        assert_eq!(result, "implement OAuth login (#123)");
    }

    #[test]
    fn test_sanitize_and_format_with_description() {
        let result = CommitMessageGenerator::sanitize_and_format(
            "add user authentication",
            Some("This feature adds OAuth support\nWith Google integration"),
            None,
        );

        assert!(result.contains("add user authentication"));
        assert!(result.contains("This feature adds OAuth support"));
        assert!(result.contains("With Google integration"));
    }

    #[test]
    fn test_sanitize_description_filters_markdown_tables() {
        let desc = "| Column 1 | Column 2 |\n|----------|----------|\n| Value 1  | Value 2  |\nRegular text here";

        let result = CommitMessageGenerator::sanitize_description(desc);

        assert!(!result.contains('|'));
        assert!(result.contains("Regular text here"));
    }
}
