from typing import TypedDict
import subprocess
from pathlib import Path

from loguru import logger
from pydantic import BaseModel


class TemporalCodingRepositoryError(Exception):
    pass


class GitRepository(BaseModel):
    name: str
    local_dir: Path

    def model_post_init(self, __context) -> None:
        """Verify the directory exists and is a valid git repository."""
        if not self.local_dir.exists():
            raise TemporalCodingRepositoryError(
                f"Repository directory does not exist: {self.local_dir}"
            )
        # Check if it's a valid git repo
        self.run_git(["rev-parse", "--is-inside-work-tree"])

    def run_git(self, args: list[str], cwd: Path | None = None) -> str:
        command = ["git"] + args
        working_dir = cwd or self.local_dir
        logger.debug(f"Running git command: {' '.join(command)} in {working_dir}")
        try:
            result = subprocess.run(
                command,
                cwd=working_dir,
                check=True,
                capture_output=True,
                text=True,
            )
            return result.stdout.strip()
        except subprocess.CalledProcessError as e:
            msg = f"Git command failed in repository '{self.name}': {e.stderr or e.stdout}"
            logger.error(msg)
            raise TemporalCodingRepositoryError(msg) from e

    def checkout(self, branch: str, create: bool = False) -> None:
        args = ["checkout", "-b", branch] if create else ["checkout", branch]
        self.chmod_777()
        self.run_git(args)

    def add(self, path: str = ".") -> None:
        self.chmod_777()
        self.run_git(["add", path])

    def commit(self, message: str) -> None:
        self.run_git(["commit", "-m", message])

    def push(self, remote: str, branch: str) -> None:
        self.run_git(["push", remote, branch])

    def rev_parse(self, ref: str = "HEAD") -> str:
        return self.run_git(["rev-parse", ref])

    @property
    def exists(self) -> bool:
        return self.local_dir.exists()

    def chmod_777(self) -> None:
        """Apply chmod -R 777 to the repository directory."""
        logger.debug(f"Applying chmod -R 777 to {self.local_dir}")
        try:
            subprocess.run(["chmod", "-R", "777", str(self.local_dir)], check=True)
        except subprocess.CalledProcessError as e:
            logger.error(f"Failed to apply chmod -R 777: {e.stderr or e.stdout}")

    def remove_non_primary(self) -> None:
        """Remove main or master branches if they exist and are not the current branch."""
        # Safety check: Ensure the local_dir is the same as the repository's top-level directory
        toplevel = self.run_git(["rev-parse", "--show-toplevel"])
        if Path(toplevel).resolve() != self.local_dir.resolve():
            raise TemporalCodingRepositoryError(
                f"Repository local_dir '{self.local_dir}' is not the top-level directory '{toplevel}'"
            )

        current_branch = self.run_git(["rev-parse", "--abbrev-ref", "HEAD"])
        # List all local branches
        branches = self.run_git(["branch", "--format=%(refname:short)"]).splitlines()
        for branch in branches:
            if branch in ["main", "master"]:
                continue
            if branch == current_branch:
                logger.warning(
                    f"Not removing '{branch}' because it is the current branch."
                )
                continue

            logger.info(f"Removing branch '{branch}' from repository '{self.name}'.")
            self.run_git(["branch", "-D", branch])


class GitRepositoryDict(TypedDict):
    name: str
    local_dir: Path
