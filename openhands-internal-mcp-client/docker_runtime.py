import asyncio
import os
import time
import uuid
from typing import Dict, List, Optional
from urllib.request import urlopen

from agents.mcp import MCPServerStreamableHttp


class DockerRuntime:
    """Context manager for running the MCP server inside a Docker container.

    Example:
        async with DockerRuntime(image_name="openhands-agent-server-rs") as server:
            agent = Agent(..., mcp_servers=[server])
            ...
    """

    def __init__(
        self,
        image_name: str,
        container_name: Optional[str] = None,
        host_port: Optional[int] = None,
        env_vars: Optional[Dict[str, str]] = None,
        volumes: Optional[Dict[str, str]] = None,
        port_mappings: Optional[List[str]] = None,
    ):
        self.image_name = image_name
        self.container_name = container_name or f"mcp-server-{uuid.uuid4().hex[:8]}"
        self.host_port = host_port
        self.env_vars = env_vars or {}
        self.volumes = volumes or {}
        self.port_mappings = port_mappings or []
        self._container_id: Optional[str] = None

    async def __aenter__(self) -> MCPServerStreamableHttp:
        # 1. Verify image exists
        proc = await asyncio.create_subprocess_exec(
            "docker",
            "inspect",
            "--type=image",
            self.image_name,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        await proc.communicate()
        if proc.returncode != 0:
            raise RuntimeError(
                f"Docker image '{self.image_name}' not found. Please build it first."
            )

        # 2. Prepare docker run command
        if self.host_port:
            port_spec = f"{self.host_port}:3000"
        else:
            port_spec = "3000"

        cmd = [
            "docker",
            "run",
            "-d",
            "--name",
            self.container_name,
            "--rm",
            "-p",
            port_spec,
        ]

        # Add environment variables
        for k, v in self.env_vars.items():
            cmd.extend(["-e", f"{k}={v}"])

        # Add volumes
        for host_path, container_path in self.volumes.items():
            # Ensure host path is absolute
            abs_host_path = os.path.abspath(host_path)
            cmd.extend(["-v", f"{abs_host_path}:{container_path}"])

        # Add extra port mappings
        for mapping in self.port_mappings:
            cmd.extend(["-p", mapping])

        cmd.append(self.image_name)

        # 3. Start container
        proc = await asyncio.create_subprocess_exec(
            *cmd, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE
        )
        stdout, stderr = await proc.communicate()
        if proc.returncode != 0:
            raise RuntimeError(f"Failed to start Docker container: {stderr.decode()}")

        self._container_id = stdout.decode().strip()

        # If host_port was not specified, find what Docker assigned
        if not self.host_port:
            proc = await asyncio.create_subprocess_exec(
                "docker",
                "port",
                self.container_name,
                "3000",
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            stdout, _ = await proc.communicate()
            if proc.returncode != 0:
                raise RuntimeError("Failed to get assigned port from Docker.")
            # stdout is something like "0.0.0.0:49483\n:::49483"
            for line in stdout.decode().splitlines():
                if ":" in line:
                    self.host_port = int(line.split(":")[-1])
                    break
            if not self.host_port:
                raise RuntimeError("Could not determine assigned port from Docker.")

        print(
            f"🚀 Started Docker container '{self.container_name}' on port {self.host_port}."
        )

        # 4. Wait for healthy
        await self._wait_for_health()

        # 5. Return MCP server instance
        mcp_url = f"http://localhost:{self.host_port}/mcp"
        self._mcp_server = MCPServerStreamableHttp(
            name="Docker MCP Server",
            params={
                "url": mcp_url,
                "timeout": 15,
            },
            cache_tools_list=False,
        )
        return await self._mcp_server.__aenter__()

    async def _wait_for_health(self, timeout: float = 30.0):
        """Wait for the server to respond to health checks."""
        print("⏳ Waiting for server to become healthy...")
        start_time = time.time()
        health_url = f"http://localhost:{self.host_port}/health"

        while time.time() - start_time < timeout:
            try:
                # We use a synchronous check in a thread or just a simple async fetch
                # For simplicity, using loop.run_in_executor with urlopen
                loop = asyncio.get_running_loop()

                def check():
                    with urlopen(health_url, timeout=1) as response:
                        return response.getcode() == 200

                if await loop.run_in_executor(None, check):
                    print("✅ Server is healthy!")
                    return
            except Exception:
                pass
            await asyncio.sleep(1)

        # If we get here, it timed out. Try to get logs for debugging.
        proc = await asyncio.create_subprocess_exec(
            "docker",
            "logs",
            self.container_name,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, _ = await proc.communicate()
        print(f"❌ Server failed to become healthy. Logs:\n{stdout.decode()}")
        raise RuntimeError("Server failed to become healthy in time.")

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        if hasattr(self, "_mcp_server"):
            await self._mcp_server.__aexit__(exc_type, exc_val, exc_tb)
        if self._container_id:
            print(
                f"🛑 Stopping and removing Docker container '{self.container_name}'..."
            )
            proc = await asyncio.create_subprocess_exec(
                "docker",
                "stop",
                self.container_name,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            await proc.communicate()
            self._container_id = None
            print("👋 Container stopped.")
