from pydantic import BaseModel


class Topic(BaseModel):
    title: str
    description: str
