use axum::Router;
use coder_mcp::runtime::bash::BashEventService;
use coder_mcp::service::CoderMcpService;
use rmcp::model::CallToolRequestParam;
use rmcp::service::ServiceExt;
use rmcp::transport::{
    streamable_http_client::StreamableHttpClientTransport,
    streamable_http_server::{session::local::LocalSessionManager, tower::StreamableHttpService},
    StreamableHttpServerConfig,
};
use serde_json::json;
use tempfile::tempdir;
use tokio::net::TcpListener;

#[tokio::test]
async fn test_http_transport_client() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}/mcp", addr);

    tokio::spawn(async move {
        let bash_events_dir = tempdir().unwrap();
        let workspace_dir = tempdir().unwrap();

        let bash_service = BashEventService::new(
            bash_events_dir.path().to_path_buf(),
            Some(workspace_dir.path().to_path_buf()),
        );
        let coder_service = CoderMcpService::new(bash_service, workspace_dir.path().to_path_buf());

        let mcp_service = StreamableHttpService::new(
            move || Ok(coder_service.clone()),
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig::default(),
        );

        let app = Router::new().nest_service("/mcp", mcp_service);
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Client
    let transport = StreamableHttpClientTransport::from_uri(url);
    let client = ().serve(transport).await.expect("Failed to connect client");

    // Call tool
    let result = client
        .call_tool(CallToolRequestParam {
            name: "execute_bash".into(),
            arguments: Some(
                json!({ "command": "echo hello_via_http" })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        })
        .await
        .expect("Failed to call tool");

    let output = result.content.first().unwrap();
    if let Some(text_content) = output.as_text() {
        assert!(text_content.text.contains("hello_via_http"));
    } else {
        panic!("Expected text output");
    }
}
