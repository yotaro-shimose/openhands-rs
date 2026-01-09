import tempfile
import shutil
import subprocess
from pathlib import Path
from typing import Optional, Dict


class TempWorkspace:
    def __init__(
        self,
        template_dir: Path,
        injections: Optional[Dict[Path, str]] = None,
        prefix: str = "workspace_",
    ):
        """
        Args:
            template_dir: The source directory to copy as the base template.
            injections: A dictionary mapping source paths (absolute) to destination paths RELATIVE to the workspace root.
            prefix: Prefix for the temporary directory.
        """
        self.template_dir = template_dir
        self.injections = injections or {}
        self.prefix = prefix
        self.temp_dir: Optional[Path] = None

    def __enter__(self) -> Path:
        # 1. Create Temp Directory
        self.temp_dir = Path(tempfile.mkdtemp(prefix=self.prefix))

        # 2. Copy Template
        if self.template_dir.exists():
            shutil.copytree(self.template_dir, self.temp_dir, dirs_exist_ok=True)

        # 3. Injections
        for src, dest_rel in self.injections.items():
            dest = self.temp_dir / dest_rel

            # Clean up destination if it exists (overwrite)
            if dest.exists():
                if dest.is_dir():
                    shutil.rmtree(dest)
                else:
                    dest.unlink()

            dest.parent.mkdir(parents=True, exist_ok=True)

            if src.is_dir():
                shutil.copytree(src, dest, dirs_exist_ok=True)
            else:
                shutil.copy2(src, dest)

        # 4. Git Init (Auto-init if not present)
        # This is often needed for GitRepository to work
        if not (self.temp_dir / ".git").exists():
            subprocess.run(
                ["git", "init"], cwd=str(self.temp_dir), check=True, capture_output=True
            )
            # Configure git to avoid errors
            subprocess.run(
                ["git", "config", "user.email", "bot@example.com"],
                cwd=str(self.temp_dir),
                check=False,
                capture_output=True,
            )
            subprocess.run(
                ["git", "config", "user.name", "Bot"],
                cwd=str(self.temp_dir),
                check=False,
                capture_output=True,
            )

            # Initial commit to have a HEAD
            subprocess.run(
                ["git", "add", "."],
                cwd=str(self.temp_dir),
                check=False,
                capture_output=True,
            )
            subprocess.run(
                ["git", "commit", "-m", "Initial commit from TempWorkspace"],
                cwd=str(self.temp_dir),
                check=False,
                capture_output=True,
            )

        # 5. CHMOD 777 for Docker
        # Essential for Docker containers to read/write mapped volumes without permission issues
        subprocess.run(["chmod", "-R", "777", str(self.temp_dir)], check=False)

        return self.temp_dir

    def __exit__(self, exc_type, exc_val, exc_tb):
        if self.temp_dir and self.temp_dir.exists():
            shutil.rmtree(self.temp_dir)
