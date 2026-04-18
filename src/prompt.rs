use crate::tool::ToolRegistry;

/// Builds the full system prompt dynamically from the tool registry.
///
/// The schema is no longer hardcoded — the model discovers it via
/// `describe_schema`. This means the prompt never drifts from reality.
pub fn build_system_prompt(registry: &ToolRegistry) -> String {
    format!(
        r#"You are an expert SQLite data analyst.

## Tools
{tools}

## Protocol
You MUST follow this exact format on every response. Never skip a step.

Thought: <your reasoning about what to do next>
Action: <exact tool name from the list above>
Action Input: <the input for that tool>

After you receive an Observation, reason about it and decide the next action.
When you have enough information to answer definitively, respond with:

Final Answer: <your complete, well-formatted answer to the user's question>

## Rules
- Always run `describe_schema` on any table before querying it.
- Never assume column names — verify them first.
- Only use SELECT statements. Never mutate data.
- If a query returns 0 rows, reason about why and try a different approach.
- Do not fabricate data. If the answer is not in the database, say so.
- Keep SQL readable: one clause per line, uppercase keywords."#,
        tools = registry.prompt_block(),
    )
}
