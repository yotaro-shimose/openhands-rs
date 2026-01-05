from typing import Literal
from pydantic import BaseModel, Field


# --- Data Structures ---
class Exercise(BaseModel):
    id: str = Field(description="Unique snake_case identifier for the exercise")
    topic: str = Field(description="Short title of the exercise")
    concept: str = Field(description="Specific API or logic to be mastered")
    rationale: str = Field(description="Why this is important and verification method")
    complexity: Literal["Beginner", "Intermediate", "Advanced"]
    api_surface: list[str] = Field(description="List of primary functions/traits used")
    source_reference: str = Field(description="Path to the defining .rs file")


class CurriculumAbstract(BaseModel):
    title: str = Field(description="Title of the module/topic")
    description: str = Field(description="Overview of coverage and prerequisites")
    exercises: list[Exercise] = Field(description="List of exercises in this module")


# --- Prompts ---
SYLLABUS_WORKER_PROMPT = """\
**Role:** You are an Expert Rust Pedagogue and Curriculum Architect.
**Objective:** Analyze the provided Rust source code and extract a "Curriculum Abstract".

**Task:**
1. Identify the **public API surface** (structs, functions, traits) in the provided code.
2. Design 1-3 specific exercises that teach a user how to use these APIs.
3. Ensure each exercise is **verifiable** via compilation and unit tests (CPU-only).
4. Ignore internal/private logic unless it's critical for understanding the public API.

**Output:**
Return a structured `CurriculumAbstract` object.
"""
