use crate::events::{Event, MessageEvent};
use crate::llm::LLM;
use crate::prompts::SYSTEM_PROMPT;
use crate::runtime::Runtime;
use genai::chat::{ChatMessage, ChatRole, ContentPart, ToolCall, ToolResponse};

pub struct Agent {
    llm: LLM,
    system_message: String,
}

impl Agent {
    pub fn new(llm: LLM, system_message: String) -> Self {
        let combined_system = format!("{}\n\n{}", SYSTEM_PROMPT, system_message);
        Self {
            llm,
            system_message: combined_system,
        }
    }

    pub async fn step(
        &self,
        history: &[Event],
        runtime: &mut dyn Runtime,
    ) -> Result<Event, Box<dyn std::error::Error + Send + Sync>> {
        let mut messages = vec![ChatMessage::system(self.system_message.clone())];

        for event in history {
            match event {
                Event::Message(m) => {
                    if m.source == "user" {
                        messages.push(ChatMessage::user(m.content.clone()));
                    } else {
                        messages.push(ChatMessage::assistant(m.content.clone()));
                    }
                }
                Event::Action(a) => {
                    let mut parts = vec![];
                    if let Some(thought) = &a.thought {
                        parts.push(ContentPart::Text(thought.clone()));
                    }
                    parts.push(ContentPart::ToolCall(ToolCall {
                        call_id: a.tool_call_id.clone(),
                        fn_name: a.tool_name.clone(),
                        fn_arguments: a.arguments.clone(),
                    }));
                    messages.push(ChatMessage {
                        role: ChatRole::Assistant,
                        content: parts.into(),
                        options: None,
                    });
                }
                Event::Observation(o) => {
                    messages.push(ChatMessage::from(ToolResponse::new(
                        o.tool_call_id.clone(),
                        o.content.clone(),
                    )));
                }
            }
        }

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

            if !response.tool_calls.is_empty() {
                let mut assistant_parts = vec![];
                if !response.content.is_empty() {
                    assistant_parts.push(ContentPart::Text(response.content.clone()));
                }

                for tool_call in &response.tool_calls {
                    assistant_parts.push(ContentPart::ToolCall(tool_call.clone()));
                }

                current_messages.push(ChatMessage {
                    role: ChatRole::Assistant,
                    content: assistant_parts.into(),
                    options: None,
                });

                for tool_call in &response.tool_calls {
                    let fn_name = &tool_call.fn_name;
                    let fn_args = tool_call.fn_arguments.clone();

                    println!(
                        "Agent executing tool: {} with args: {}",
                        fn_name,
                        fn_args.to_string()
                    );

                    let result = runtime.execute(fn_name, fn_args).await;
                    let output_content = match result {
                        Ok(s) => s,
                        Err(e) => format!("Error: {}", e),
                    };

                    println!("Agent tool output: {}", output_content);

                    current_messages.push(ChatMessage::from(ToolResponse::new(
                        tool_call.call_id.clone(),
                        output_content,
                    )));
                }
            } else {
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

    #[tokio::test]
    async fn test_agent_step() {
        dotenv::dotenv().ok();
        let api_key = std::env::var("OPENAI_API_KEY").ok();
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
        use crate::runtime::LocalRuntime;
        let mut runtime = LocalRuntime::new(vec![]);

        let history = vec![Event::Message(MessageEvent {
            source: "user".to_string(),
            content: "Hello".to_string(),
        })];

        let event = agent
            .step(&history, &mut runtime)
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
        use crate::runtime::LocalRuntime;
        let mut runtime = LocalRuntime::new(vec![Box::new(CmdTool)]);

        // Request that requires tool execution
        let history = vec![Event::Message(MessageEvent {
            source: "user".to_string(),
            content: "Execute 'echo hello_world' using the cmd tool.".to_string(),
        })];

        let event = agent
            .step(&history, &mut runtime)
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
