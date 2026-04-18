use chrono::Local;

use crate::{
    llm::{LlmProvider, Provider},
    tool::ToolRegistry,
};

use super::trace::AgentStep;

const MAX_STEPS: usize = 10;
// Stop the LLM mid-stream the moment it would write the Observation itself.
// We supply the observation — the model must not fabricate it.
const STOP_SEQUENCES: &[&str] = &["Observation:"];

const CORRECTIVE_OBSERVATION: &str = "Your response did not follow the required format. \
     Use exactly one of:\n\
     • Thought: <reasoning>\n\
     • Action: <tool_name>\\nAction Input: <input>\n\
     • Final Answer: <answer>";

/// Drives the ReAct loop: Thought → Action → Observation → … → Final Answer.
pub struct ReActRunner {
    llm: Provider,
    tools: ToolRegistry,
    system_prompt: String,
}

impl ReActRunner {
    pub fn new(llm: Provider, tools: ToolRegistry, system_prompt: String) -> Self {
        Self {
            llm,
            tools,
            system_prompt,
        }
    }

    pub async fn run(&self, question: &str) -> String {
        let mut history = self.initial_history(question);
        let mut final_answer = String::from("Agent did not produce a final answer.");

        for step in 1..=MAX_STEPS {
            println!("\n── Step {step}/{MAX_STEPS} ──────────────────────────");

            let response = match self.llm.complete(&history, STOP_SEQUENCES).await {
                Ok(r) if r.trim().is_empty() => {
                    eprintln!("[Runner] Empty LLM response — stopping.");
                    break;
                }
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[Runner] LLM error: {e}");
                    break;
                }
            };

            // Append the raw response to history before we parse it,
            // so the model always sees its own prior output.
            history.push_str(&response);

            let parsed = AgentStep::parse(&response);
            println!("[Agent] {parsed:?}");

            let observation = match parsed {
                AgentStep::FinalAnswer(answer) => {
                    final_answer = answer;
                    break;
                }

                AgentStep::Action {
                    ref tool,
                    ref input,
                } => {
                    println!("[Runner] Invoking tool '{tool}' with input: {input}");
                    self.tools.invoke(tool, input).await
                }

                AgentStep::Thought(_) => {
                    // Pure reasoning step — no tool call, no observation needed.
                    // Let the model continue on the next iteration.
                    continue;
                }

                AgentStep::Malformed(_) => CORRECTIVE_OBSERVATION.to_owned(),
            };

            println!("[Observation] {observation}");
            history.push_str(&format!("\nObservation: {observation}\n"));
        }

        final_answer
    }

    fn initial_history(&self, question: &str) -> String {
        let date = Local::now().format("%Y-%m-%d");
        format!(
            "{system_prompt}\nCurrent Date: {date}\n\nQuestion: {question}\n",
            system_prompt = self.system_prompt,
        )
    }
}
