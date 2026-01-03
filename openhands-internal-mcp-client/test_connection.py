import asyncio
import httpx
import json


async def test_post_initialize():
    url = "http://localhost:3000/mcp"

    # Construct minimal InitializeRequest
    init_req = {
        "jsonrpc": "2.0",
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test-client", "version": "1.0"},
        },
        "id": 1,
    }

    print(f"Sending POST to {url} with InitializeRequest...")
    async with httpx.AsyncClient() as client:
        # We need to set Accept header for SSE?
        # rmcp checks: "Client must accept text/event-stream"
        headers = {
            "Accept": "application/json, text/event-stream",
            "Content-Type": "application/json",
        }
        response = await client.post(url, json=init_req, headers=headers)

        print(f"Status: {response.status_code}")
        print(f"Headers: {response.headers}")
        print(f"Content: {response.text[:200]}...")


if __name__ == "__main__":
    asyncio.run(test_post_initialize())
