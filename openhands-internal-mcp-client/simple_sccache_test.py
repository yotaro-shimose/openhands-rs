#!/usr/bin/env python3
"""Simple direct test of sccache functionality in RustCodingEnvironment."""

import asyncio
import subprocess
from pathlib import Path
from rust_env import RustCodingEnvironment


async def main():
    cache_dir = Path.cwd() / ".sccache_direct_test"
    cargo_cache_dir = Path.cwd() / ".cargo_cache_direct_test"
    workspace_dir = Path.cwd() / "rust_direct_test_workspace"

    print("=== Direct sccache Test ===\n")

    async with RustCodingEnvironment(
        cache_dir=cache_dir,
        cargo_cache_dir=cargo_cache_dir,
        workspace_dir=workspace_dir,
    ) as server:
        # Access the container details from the RustCodingEnvironment instance
        container_name = server.container_name
        print(f"Container: {container_name}\n")

        # Execute commands directly in the container
        container_id = server._container_id

        print("1. Checking environment...")
        result = subprocess.run(
            [
                "docker",
                "exec",
                container_id,
                "bash",
                "-c",
                "env | grep -E '(RUSTC_WRAPPER|SCCACHE|CARGO)'",
            ],
            capture_output=True,
            text=True,
        )
        print(result.stdout)

        print("\n2. Checking sccache access...")
        result = subprocess.run(
            ["docker", "exec", container_id, "sccache", "--version"],
            capture_output=True,
            text=True,
        )
        print(result.stdout)

        print("\n3. Creating a simple Rust project...")
        result = subprocess.run(
            [
                "docker",
                "exec",
                "-w",
                "/workspace",
                container_id,
                "cargo",
                "new",
                "hello_test",
                "--quiet",
            ],
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            print(f"Error: {result.stderr}")

        print("\n4. Building the project (first time)...")
        result = subprocess.run(
            [
                "docker",
                "exec",
                "-w",
                "/workspace/hello_test",
                container_id,
                "cargo",
                "build",
            ],
            capture_output=True,
            text=True,
        )
        print("Build output (last 10 lines):")
        print("\n".join(result.stderr.splitlines()[-10:]))

        print("\n5. Checking sccache stats after first build...")
        result = subprocess.run(
            ["docker", "exec", container_id, "sccache", "--show-stats"],
            capture_output=True,
            text=True,
        )
        print(result.stdout)

        print("\n6. Cleaning and rebuilding (second time)...")
        result = subprocess.run(
            [
                "docker",
                "exec",
                "-w",
                "/workspace/hello_test",
                container_id,
                "cargo",
                "clean",
            ],
            capture_output=True,
            text=True,
        )
        result = subprocess.run(
            [
                "docker",
                "exec",
                "-w",
                "/workspace/hello_test",
                container_id,
                "cargo",
                "build",
            ],
            capture_output=True,
            text=True,
        )
        print("Build output (last 10 lines):")
        print("\n".join(result.stderr.splitlines()[-10:]))

        print("\n7. Checking sccache stats after second build...")
        result = subprocess.run(
            ["docker", "exec", container_id, "sccache", "--show-stats"],
            capture_output=True,
            text=True,
        )
        print(result.stdout)

        print("\n8. Checking Cargo cache on host...")
        registry_files = list((cargo_cache_dir / "registry").rglob("*"))
        print(f"Registry files found: {len(registry_files)}")
        if registry_files:
            print(f"Sample: {registry_files[0]}")

    print("\n✅ Test complete!")


if __name__ == "__main__":
    asyncio.run(main())
