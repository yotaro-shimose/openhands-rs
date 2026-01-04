from openhands_agent.exam.repository import GitRepository, GitRepositoryDict
from typing import TypedDict
from pydantic import BaseModel


class CodingExam(BaseModel):
    id: str
    image_name: str
    project: GitRepository
    library: GitRepository
    solution_commit: str
    problem_commit: str
    question: str
    eval_rubric: str


class CodingExamDict(TypedDict):
    id: str
    image_name: str
    project: GitRepositoryDict
    library: GitRepositoryDict
    solution_commit: str
    problem_commit: str
    question: str
