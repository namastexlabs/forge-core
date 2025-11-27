/// Commit message validator for quality assurance
pub struct CommitValidator;

#[derive(Debug, Clone)]
pub struct ValidationWarning {
    pub message: String,
    pub severity: WarningSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningSeverity {
    Info,
    Warning,
    Error,
}

impl CommitValidator {
    /// Validate commit message and return warnings (if any)
    pub fn validate(commit_message: &str) -> Vec<ValidationWarning> {
        let mut warnings = Vec::new();

        // Check for conversational patterns (ERROR level)
        if Self::has_conversational_pattern(commit_message) {
            warnings.push(ValidationWarning {
                message: "Commit message contains conversational AI patterns (e.g., 'Perfect!', 'Let me')".to_string(),
                severity: WarningSeverity::Error,
            });
        }

        // Check for excessive length (WARNING level)
        let first_line = commit_message.lines().next().unwrap_or("");
        if first_line.len() > 72 {
            warnings.push(ValidationWarning {
                message: format!(
                    "Subject line is {} characters (recommended: 50, max: 72)",
                    first_line.len()
                ),
                severity: WarningSeverity::Warning,
            });
        }

        // Check for internal UUIDs (WARNING level)
        if commit_message.contains("automagik-forge") {
            warnings.push(ValidationWarning {
                message: "Commit message contains internal identifier 'automagik-forge'".to_string(),
                severity: WarningSeverity::Warning,
            });
        }

        // Check for markdown tables/excessive formatting (INFO level)
        if commit_message.contains('|') && commit_message.lines().count() > 3 {
            warnings.push(ValidationWarning {
                message: "Commit message contains markdown tables or excessive formatting"
                    .to_string(),
                severity: WarningSeverity::Info,
            });
        }

        // Check for missing GitHub issue reference (INFO level)
        if !Self::has_issue_reference(commit_message) {
            warnings.push(ValidationWarning {
                message: "Consider adding GitHub issue reference (e.g., '#123')".to_string(),
                severity: WarningSeverity::Info,
            });
        }

        warnings
    }

    /// Check if commit follows conventional commits format loosely
    pub fn follows_conventional_commits(commit_message: &str) -> bool {
        let first_line = commit_message.lines().next().unwrap_or("");

        let conventional_prefixes = [
            "feat:", "fix:", "docs:", "style:", "refactor:", "perf:", "test:", "build:", "ci:",
            "chore:", "revert:",
            "feat(", "fix(", "docs(", "style(", "refactor(", "perf(", "test(", "build(", "ci(",
            "chore(", "revert(",
        ];

        conventional_prefixes
            .iter()
            .any(|prefix| first_line.starts_with(prefix))
    }

    /// Check for conversational patterns
    fn has_conversational_pattern(msg: &str) -> bool {
        let conversational_patterns = [
            "Perfect!",
            "Good, I",
            "Good,",
            "Let me",
            "I'll",
            "I will",
            "I can see",
            "Sure,",
            "Okay,",
            "Great!",
        ];

        let first_line = msg.lines().next().unwrap_or("");

        conversational_patterns
            .iter()
            .any(|pattern| first_line.starts_with(pattern))
    }

    /// Check if message has GitHub issue reference
    fn has_issue_reference(msg: &str) -> bool {
        msg.contains("#") && msg.chars().any(|c| c.is_ascii_digit())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_good_message() {
        let msg = "feat: add user authentication (#123)";
        let warnings = CommitValidator::validate(msg);

        // Should have no errors or warnings, just optional info
        assert!(warnings
            .iter()
            .all(|w| w.severity != WarningSeverity::Error));
    }

    #[test]
    fn test_validate_conversational_message() {
        let msg = "Perfect! Let me create a summary for you:";
        let warnings = CommitValidator::validate(msg);

        assert!(warnings
            .iter()
            .any(|w| w.severity == WarningSeverity::Error));
    }

    #[test]
    fn test_validate_long_subject() {
        let msg = "a".repeat(100);
        let warnings = CommitValidator::validate(&msg);

        assert!(warnings
            .iter()
            .any(|w| w.severity == WarningSeverity::Warning && w.message.contains("characters")));
    }

    #[test]
    fn test_follows_conventional_commits() {
        assert!(CommitValidator::follows_conventional_commits(
            "feat: add feature"
        ));
        assert!(CommitValidator::follows_conventional_commits(
            "fix(auth): correct login bug"
        ));
        assert!(!CommitValidator::follows_conventional_commits(
            "Perfect! Let me help"
        ));
        assert!(!CommitValidator::follows_conventional_commits(
            "random commit message"
        ));
    }

    #[test]
    fn test_has_issue_reference() {
        assert!(CommitValidator::has_issue_reference(
            "fix: bug (#123)"
        ));
        assert!(CommitValidator::has_issue_reference(
            "feat: feature\n\nCloses #456"
        ));
        assert!(!CommitValidator::has_issue_reference("fix: bug"));
    }
}
