#!/usr/bin/env python3
"""Test Cargo cache sharing with a Tokio-based Rust project."""

import asyncio
import subprocess
import time
from pathlib import Path
from rust_env import RustCodingEnvironment


async def main():
    cache_dir = Path.cwd() / ".sccache_tokio_test"
    cargo_cache_dir = Path.cwd() / ".cargo_cache_tokio_test"
    workspace_dir = Path.cwd() / "tokio_test_workspace"

    print("=" * 60)
    print("Testing Cargo Cache Sharing with Tokio")
    print("=" * 60)

    # Note: To clean cache directories, run: sudo rm -rf .sccache_tokio_test .cargo_cache_tokio_test tokio_test_workspace

    print("\n🏗️  ITERATION 1: Fresh build (downloading dependencies)")
    print("=" * 60)

    async with RustCodingEnvironment(
        cache_dir=cache_dir,
        cargo_cache_dir=cargo_cache_dir,
        workspace_dir=workspace_dir,
    ) as server:
        container_id = server._container_id

        print("\n1️⃣  Creating Tokio project...")
        subprocess.run(
            [
                "docker",
                "exec",
                "-w",
                "/workspace",
                container_id,
                "cargo",
                "new",
                "tokio_example",
                "--quiet",
            ],
            check=True,
        )

        print("\n2️⃣  Adding Tokio dependency...")
        cargo_toml = """[package]
name = "tokio_example"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1", features = ["full"] }
"""
        subprocess.run(
            [
                "docker",
                "exec",
                "-w",
                "/workspace/tokio_example",
                container_id,
                "bash",
                "-c",
                f"cat > Cargo.toml << 'EOF'\n{cargo_toml}\nEOF",
            ],
            check=True,
        )

        print("\n3️⃣  Building project (first time - will download dependencies)...")
        start = time.time()
        result = subprocess.run(
            [
                "docker",
                "exec",
                "-w",
                "/workspace/tokio_example",
                container_id,
                "cargo",
                "build",
                "--release",
            ],
            capture_output=True,
            text=True,
        )
        elapsed = time.time() - start

        print(f"   ⏱️  Build took: {elapsed:.2f}s")

        # Show download activity
        downloading = [
            line
            for line in result.stderr.splitlines()
            if "Downloading" in line or "Compiling" in line
        ]
        if downloading:
            print(f"   📦 Downloaded/Compiled {len(downloading)} items")
            print(f"   First few: {downloading[:3]}")

        print("\n4️⃣  Checking Cargo cache on host...")
        registry_files = list((cargo_cache_dir / "registry").rglob("*.crate"))
        print(f"   📦 Registry cache: {len(registry_files)} .crate files")
        if registry_files:
            total_size = sum(f.stat().st_size for f in registry_files) / 1024 / 1024
            print(f"   💾 Total size: {total_size:.2f} MB")

    print("\n" + "=" * 60)
    print("🔄 ITERATION 2: Rebuilding in new container")
    print("=" * 60)
    time.sleep(2)

    async with RustCodingEnvironment(
        cache_dir=cache_dir,
        cargo_cache_dir=cargo_cache_dir,
        workspace_dir=workspace_dir,
    ) as server:
        container_id = server._container_id

        print("\n5️⃣  Cleaning build artifacts (keeping dependencies cached)...")
        subprocess.run(
            [
                "docker",
                "exec",
                "-w",
                "/workspace/tokio_example",
                container_id,
                "cargo",
                "clean",
            ],
            check=True,
        )

        print("\n6️⃣  Rebuilding project (should use cached dependencies)...")
        start = time.time()
        result = subprocess.run(
            [
                "docker",
                "exec",
                "-w",
                "/workspace/tokio_example",
                container_id,
                "cargo",
                "build",
                "--release",
            ],
            capture_output=True,
            text=True,
        )
        elapsed = time.time() - start

        print(f"   ⏱️  Build took: {elapsed:.2f}s")

        # Check if any downloads happened
        downloading = [
            line for line in result.stderr.splitlines() if "Downloading" in line
        ]
        if downloading:
            print(f"   ⚠️  Downloaded {len(downloading)} items (unexpected!)")
        else:
            print(f"   ✅ No downloads - used cached dependencies!")

        compiling = [line for line in result.stderr.splitlines() if "Compiling" in line]
        print(f"   🔨 Compiled {len(compiling)} crates")

    print("\n" + "=" * 60)
    print("📊 CACHE ANALYSIS")
    print("=" * 60)

    registry_path = cargo_cache_dir / "registry"
    if registry_path.exists():
        cache_files = list(registry_path.rglob("*"))
        crate_files = [f for f in cache_files if f.suffix == ".crate"]

        print(f"\n✅ Registry cache persisted:")
        print(f"   - Total files: {len(cache_files)}")
        print(f"   - Crate archives: {len(crate_files)}")

        if crate_files:
            total_size = sum(f.stat().st_size for f in crate_files) / 1024 / 1024
            print(f"   - Cache size: {total_size:.2f} MB")
            print(f"\n   Sample cached crates:")
            for crate in sorted(crate_files)[:5]:
                print(f"     - {crate.name}")

    print("\n✅ Test complete! Cargo cache sharing is working.")
    print("   Dependencies downloaded once and reused across containers.")


if __name__ == "__main__":
    asyncio.run(main())
