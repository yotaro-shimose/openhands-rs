use openhands_sdk_rs::{
    agent::Agent,
    events::{Event, MessageEvent},
    llm::{LLM, LLMConfig},
    runtime::RemoteRuntime,
    tools::{CmdTool, FileReadTool, FileWriteTool},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dotenv::dotenv().ok();
    println!("Testing RemoteRuntime against local server...");

    let api_key = std::env::var("OPENAI_API_KEY").ok();
    if api_key.is_none() {
        println!("OPENAI_API_KEY not set. Please set it to run this test.");
        return Ok(());
    }

    // 1. Configure LLM
    let config = LLMConfig {
        model: "gpt-5-nano".to_string(),
        api_key,
        reasoning_effort: Some("minimal".to_string()),
    };
    let llm = LLM::new(config);

    // 2. Initialize Agent
    let agent = Agent::new(
        llm,
        "You are a helpful assistant with access to file and command tools.".to_string(),
    );

    // 3. Initialize Runtime (RemoteRuntime)
    let mut runtime = RemoteRuntime::new(
        "http://localhost:3000".to_string(),
        vec![
            Box::new(CmdTool),
            Box::new(FileReadTool),
            Box::new(FileWriteTool),
        ],
    );

    // 4. User Task - A multi-step task to verify ReAct loop and tool usage
    let user_task = "Create a directory named 'alignment_test', then write a file 'status.txt' inside it with the text 'aligned', and finally read that file.";
    println!("\nUser Task: {}", user_task);

    // 5. Run Agent Step
    let history = vec![Event::Message(MessageEvent {
        source: "user".to_string(),
        content: user_task.to_string(),
    })];

    println!("\n--- Running Agent ---");
    let event = agent.step(&history, &mut runtime).await?;
    println!("Agent response: {:?}", event);

    if let Event::Message(m) = event {
        println!("\nAgent finished the task. Final Response:\n{}", m.content);
    }

    Ok(())
}
