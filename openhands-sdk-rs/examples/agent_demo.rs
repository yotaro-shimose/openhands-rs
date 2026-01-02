use openhands_sdk_rs::{
    agent::Agent,
    events::{Event, MessageEvent},
    llm::{LLM, LLMConfig},
    runtime::DockerRuntime,
    tools::{CmdTool, FileReadTool, FileWriteTool},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dotenv::dotenv().ok();
    openhands_sdk_rs::logger::init_logging();
    println!("Initializing Coding Agent...");

    let api_key = std::env::var("OPENAI_API_KEY").ok();
    if api_key.is_none() {
        println!("OPENAI_API_KEY not set. Please set it to run this example.");
        return Ok(());
    }

    println!("Initializing Coding Agent...");

    // 1. Configure LLM
    let config = LLMConfig {
        model: "gpt-5-nano".to_string(),
        api_key,
        reasoning_effort: Some("minimal".to_string()),
    };
    let llm = LLM::new(config);

    // 2. Initialize Agent with System Prompt
    let agent = Agent::new(
        llm,
        "You are a skilled Python coding assistant. You can write files and execute commands."
            .to_string(),
    );

    // 3. Initialize Runtime (DockerRuntime)
    //    We separate execution from decision making. The runtime holds the tools.
    let mut runtime = DockerRuntime::new(
        "openhands-agent-server-rs:latest",
        vec![
            Box::new(CmdTool),
            Box::new(FileReadTool),
            Box::new(FileWriteTool),
        ],
    );

    // 4. Define the Task
    let task = "Write a Python script named 'hello.py' that prints 'Hello from Rust Agent!', then execute it.";
    println!("\nUser Task: {}", task);

    let history = vec![Event::Message(MessageEvent {
        source: "user".to_string(),
        content: task.to_string(),
    })];

    // 5. Run Step
    // In a real app, this would be a loop. For this demo, we run one 'step'
    // which includes the internal ReAct loop (Think -> Tool -> Output -> Answer).
    let response_event = agent.step(&history, &mut runtime).await?;

    if let Event::Message(m) = response_event {
        println!("\nAgent Final Response:\n{}", m.content);
    } else {
        println!("\nAgent returned non-message event.");
    }

    Ok(())
}
