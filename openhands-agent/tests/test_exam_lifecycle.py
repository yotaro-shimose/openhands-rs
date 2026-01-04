import pytest
from openhands_agent.exam.creator import create_exam
from openhands_agent.exam.runner import solve_exam, evaluate_exam
from openhands_agent.exam.topic import Topic


from tests.utils import setup_test_repos


@pytest.mark.asyncio
async def test_create_exam_live(tmp_path):
    """Live test of create_exam with real agent execution."""
    project_repo, lib_repo = setup_test_repos(tmp_path / "test_repos")

    # A simple topic that requires using the library
    topic = Topic(
        title="Return True",
        description=(
            "Modify the function `solution` in `src/solution.rs` to return `true`.\n"
            "The function signature should be `pub fn solution() -> bool`."
        ),
    )

    # Execute create_exam (this runs the real agent)
    # We expect this to take some time and consume tokens
    exam = await create_exam(project_repo, lib_repo, topic)

    # Basic Validations
    print(f"Exam ID: {exam.id}")
    print(f"Question: {exam.question}")

    assert "Return True" in exam.question or "true" in exam.question.lower()
    assert len(exam.eval_rubric) > 10
    assert exam.problem_commit != exam.solution_commit

    # Verify Git History Relationship
    repo = exam.project
    # solution_commit should be a descendant of problem_commit
    try:
        repo.run_git(
            ["merge-base", "--is-ancestor", exam.problem_commit, exam.solution_commit]
        )
    except Exception:
        pytest.fail("Problem commit is NOT an ancestor of Solution commit")

    # Verify Content in Solution State (HEAD)
    # HEAD should have the full solution
    head_files = repo.run_git(["ls-tree", "-r", "HEAD", "--name-only"]).splitlines()
    assert "src/solution.rs" in head_files
    # Check content contains implementation
    sol_content = (repo.local_dir / "src/solution.rs").read_text()
    assert "true" in sol_content

    # Verify Content in Problem State
    # Checkout problem commit and check files
    repo.run_git(["checkout", exam.problem_commit])
    prob_content = (repo.local_dir / "src/solution.rs").read_text()
    # Should be stubbed or empty
    assert "true" not in prob_content or "todo!" in prob_content

    # Verify Push to Original Repo
    branch_name = f"exam-{exam.id}"
    branches = project_repo.run_git(["branch", "--list", branch_name])
    assert branch_name in branches

    # Verify we can checkout the commits in the original repo
    project_repo.run_git(["checkout", branch_name])
    head_commit = project_repo.run_git(["rev-parse", "HEAD"])
    assert head_commit.strip() == exam.solution_commit.strip()

    # --- Test solve_exam ---
    print("\nTesting solve_exam...")
    solution_path = await solve_exam(exam)

    # Check if solution exists in the new workspace
    print(f"Solution Path: {solution_path}")
    new_sol_content = (solution_path / "src/solution.rs").read_text()
    print(f"Solution Content:\n{new_sol_content}")

    # Assert that the agent actually wrote the solution again
    assert "true" in new_sol_content

    # --- Test evaluate_exam ---
    print("\nTesting evaluate_exam...")
    report = await evaluate_exam(exam, solution_path)
    print(f"Evaluation Report:\n{report}")

    # Check for score report
    assert "TOTAL USER SCORE" in report or "points" in report.lower()
