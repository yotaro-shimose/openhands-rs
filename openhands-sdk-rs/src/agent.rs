use crate::events::{Event, MessageEvent};
use crate::llm::LLM;
use crate::runtime::Runtime;
use genai::chat::ChatMessage;

pub struct Agent {
    llm: LLM,
    system_message: String,
}

impl Agent {
    pub fn new(llm: LLM, system_message: String) -> Self {
        Self {
            llm,
            system_message,
        }
    }

    pub async fn step(
        &self,
        history: Vec<Event>,
        runtime: &mut dyn Runtime,
    ) -> Result<Event, Box<dyn std::error::Error + Send + Sync>> {
        // dynamic conversion of events to ChatMessage
        let mut messages = vec![ChatMessage::system(self.system_message.clone())];

        for event in history {
            match event {
                Event::Message(m) => {
                    if m.source == "user" {
                        messages.push(ChatMessage::user(m.content));
                    } else {
                        messages.push(ChatMessage::assistant(m.content));
                    }
                }
                _ => {} // Ignore others for basic chat
            }
        }

        // Convert Runtime Tools to genai Tools
        let genai_tools: Vec<genai::chat::Tool> = runtime
            .tools()
            .iter()
            .map(|t| genai::chat::Tool {
                name: t.name(),
                description: Some(t.description()),
                schema: Some(t.parameters()),
                config: None,
            })
            .collect();

        let tools_arg = if genai_tools.is_empty() {
            None
        } else {
            Some(genai_tools)
        };

        let mut current_messages = messages.clone();
        let max_iterations = 10;

        for _ in 0..max_iterations {
            let response = self
                .llm
                .completion(current_messages.clone(), tools_arg.clone())
                .await?;

            // If tool calls are present, execute them
            if !response.tool_calls.is_empty() {
                let tool_call = &response.tool_calls[0]; // Handle first one for now

                let assistant_msg = if !response.content.is_empty() {
                    ChatMessage::assistant(response.content.clone())
                } else {
                    ChatMessage::assistant("Calling tool...".to_string())
                };
                current_messages.push(assistant_msg);

                let fn_name = &tool_call.fn_name;
                let fn_args = tool_call.fn_arguments.clone();
                let fn_args_str = fn_args.to_string(); // For logging

                println!(
                    "Agent executing tool: {} with args: {}",
                    fn_name, fn_args_str
                );

                // Delegate execution to Runtime
                // fn_args is already an serde_json::Value here (cloned from tool_call.fn_arguments)
                let result = runtime.execute(fn_name, fn_args).await;

                let output_content = match result {
                    Ok(s) => s,
                    Err(e) => format!("Error: {}", e),
                };

                current_messages.push(ChatMessage::user(format!(
                    "Tool '{}' Output: {}",
                    fn_name, output_content
                )));
            } else {
                // No tool calls, just text response -> Done
                return Ok(Event::Message(MessageEvent {
                    source: "agent".to_string(),
                    content: response.content,
                }));
            }
        }

        Err("Max iterations reached".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::LLMConfig;
    use crate::runtime::LocalRuntime;
    use crate::tools::{CmdTool, Tool};

    #[tokio::test]
    async fn test_agent_step() {
        dotenv::dotenv().ok();
        let api_key = std::env::var("OPENAI_API_KEY").ok();
        let tools: Vec<Box<dyn Tool>> = vec![Box::new(crate::tools::CmdTool)];
        let mut runtime = crate::runtime::LocalRuntime::new(tools);
        if api_key.is_none() {
            println!("Skipping test_agent_step because OPENAI_API_KEY is not set");
            return;
        }

        let config = LLMConfig {
            model: "gpt-5-nano".to_string(),
            api_key,
            reasoning_effort: Some("minimal".to_string()),
        };
        let llm = LLM::new(config);
        let agent = Agent::new(llm, "You are a helpful assistant.".to_string());

        // Runtime
        use crate::runtime::DefaultRuntime;
        let mut runtime = DefaultRuntime::new(vec![]);

        let history = vec![Event::Message(MessageEvent {
            source: "user".to_string(),
            content: "Hello".to_string(),
        })];

        let event = agent
            .step(history, &mut runtime)
            .await
            .expect("Step failed");

        if let Event::Message(m) = event {
            assert_eq!(m.source, "agent");
            assert!(!m.content.is_empty());
            println!("Agent Response: {}", m.content);
        } else {
            panic!("Expected MessageEvent");
        }
    }

    #[tokio::test]
    async fn test_agent_tool_loop() {
        dotenv::dotenv().ok();
        let api_key = std::env::var("OPENAI_API_KEY").ok();
        if api_key.is_none() {
            return;
        }

        use crate::tools::CmdTool;
        let config = LLMConfig {
            model: "gpt-5-nano".to_string(),
            api_key,
            reasoning_effort: Some("minimal".to_string()),
        };
        let llm = LLM::new(config);
        let agent = Agent::new(
            llm,
            "You are a helpful assistant that can execute commands.".to_string(),
        );

        // Runtime with CmdTool
        use crate::runtime::DefaultRuntime;
        let mut runtime = DefaultRuntime::new(vec![Box::new(CmdTool)]);

        // Request that requires tool execution
        let history = vec![Event::Message(MessageEvent {
            source: "user".to_string(),
            content: "Execute 'echo hello_world' using the cmd tool.".to_string(),
        })];

        let event = agent
            .step(history, &mut runtime)
            .await
            .expect("Step failed");

        if let Event::Message(m) = event {
            println!("Agent Tool Response: {}", m.content);
            assert!(
                m.content.contains("hello_world") || m.content.contains("executed"),
                "Response should mention the output or action"
            );
        } else {
            panic!("Expected final MessageEvent");
        }
    }
}
