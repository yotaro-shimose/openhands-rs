from pathlib import Path
from typing import Dict, Optional, List
from docker_runtime import DockerRuntime
from agents.mcp import MCPServerStreamableHttp


class RustCodingEnvironment(DockerRuntime):
    """A specialized Docker runtime for Rust development with sccache support.

    This environment automatically configures sccache to use a persistent host directory
    and mounts a workspace directory for project files.
    """

    def __init__(
        self,
        image_name: str = "openhands-agent-server-rs",
        cache_dir: str | Path = "./.sccache",
        cargo_cache_dir: str | Path = "./.cargo_cache",
        workspace_dir: str | Path = "./workspace",
        container_name: Optional[str] = None,
        host_port: Optional[int] = None,
        env_vars: Optional[Dict[str, str]] = None,
        volumes: Optional[Dict[str, str]] = None,
        port_mappings: Optional[List[str]] = None,
    ):
        self.image_name = image_name
        self.cache_dir = Path(cache_dir).resolve()
        self.cargo_cache_dir = Path(cargo_cache_dir).resolve()
        self.workspace_dir = Path(workspace_dir).resolve()

        # Merge environment variables for Rust caching
        env = env_vars or {}
        env.setdefault("RUSTC_WRAPPER", "/usr/local/bin/sccache")
        env.setdefault("SCCACHE_DIR", "/var/cache/sccache")
        env.setdefault("CARGO_INCREMENTAL", "0")

        # Merge volume mounts
        vols = volumes or {}
        vols[str(self.cache_dir)] = "/var/cache/sccache"
        vols[str(self.cargo_cache_dir / "registry")] = "/usr/local/cargo/registry"
        vols[str(self.cargo_cache_dir / "git")] = "/usr/local/cargo/git"
        vols[str(self.workspace_dir)] = "/workspace"

        super().__init__(
            workspace_dir=str(self.workspace_dir),
            image_name=image_name,
            container_name=container_name,
            host_port=host_port,
            env_vars=env,
            volumes=vols,
            port_mappings=port_mappings,
        )

    async def __aenter__(self) -> MCPServerStreamableHttp:
        """Starts the Rust coding environment."""
        print("🦀 Initializing Rust Coding Environment...")
        return await super().__aenter__()
