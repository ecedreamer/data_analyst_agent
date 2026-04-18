/// A fully parsed step from the LLM's output.
///
/// The parser is strict: every response must be classifiable.
/// Ambiguous or malformed responses become `AgentStep::Malformed`
/// so the runner can inject a corrective observation rather than
/// silently skipping or crashing.
#[derive(Debug, PartialEq)]
pub enum AgentStep {
    /// `Thought: <text>` — reasoning, no action taken.
    Thought(String),

    /// `Action: <tool>\nAction Input: <input>` — tool invocation.
    Action { tool: String, input: String },

    /// `Final Answer: <text>` — terminal step, loop exits.
    FinalAnswer(String),

    /// Response didn't match any expected pattern.
    Malformed(String),
}

impl AgentStep {
    /// Parse one LLM response chunk into a structured step.
    ///
    /// Matching priority:
    ///   1. Final Answer  (checked first — a response can contain "Action" AND "Final Answer")
    ///   2. Action + Action Input pair
    ///   3. Thought
    ///   4. Malformed
    pub fn parse(response: &str) -> Self {
        // 1. Final Answer
        if let Some(rest) = find_tag(response, "Final Answer:") {
            return AgentStep::FinalAnswer(rest.trim().to_owned());
        }

        // 2. Action + Action Input
        if let Some(tool_line) = find_tag(response, "Action:") {
            let tool = tool_line.lines().next().unwrap_or("").trim().to_owned();
            let input = find_tag(response, "Action Input:")
                .unwrap_or("")
                .trim()
                .to_owned();
            return AgentStep::Action { tool, input };
        }

        // 3. Thought
        if let Some(text) = find_tag(response, "Thought:") {
            return AgentStep::Thought(text.trim().to_owned());
        }

        // 4. Fallback
        AgentStep::Malformed(response.trim().to_owned())
    }
}

/// Returns everything after `tag` in `text`, or `None` if the tag is absent.
fn find_tag<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    text.find(tag).map(|pos| &text[pos + tag.len()..])
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_final_answer() {
        let r = "Thought: done\nFinal Answer: 42";
        assert_eq!(AgentStep::parse(r), AgentStep::FinalAnswer("42".into()));
    }

    #[test]
    fn parses_action_with_input() {
        let r = "Thought: need data\nAction: query_database\nAction Input: SELECT 1";
        assert_eq!(
            AgentStep::parse(r),
            AgentStep::Action {
                tool: "query_database".into(),
                input: "SELECT 1".into(),
            }
        );
    }

    #[test]
    fn parses_thought_only() {
        let r = "Thought: I should check the schema first.";
        assert_eq!(
            AgentStep::parse(r),
            AgentStep::Thought("I should check the schema first.".into())
        );
    }

    #[test]
    fn parses_malformed() {
        let r = "I don't know what to do.";
        assert!(matches!(AgentStep::parse(r), AgentStep::Malformed(_)));
    }

    #[test]
    fn final_answer_takes_priority_over_action() {
        // If somehow both appear, Final Answer wins.
        let r = "Action: query_database\nAction Input: SELECT 1\nFinal Answer: done";
        assert!(matches!(AgentStep::parse(r), AgentStep::FinalAnswer(_)));
    }
}
