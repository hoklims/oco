"""Configuration for the ML worker."""

from __future__ import annotations

from pydantic import BaseModel, Field


class ModelConfig(BaseModel):
    """Configuration for ML models."""

    embedding_model: str = Field(
        default="all-MiniLM-L6-v2",
        description="Sentence Transformers model name for embeddings",
    )
    reranker_model: str = Field(
        default="cross-encoder/ms-marco-MiniLM-L-6-v2",
        description="Cross-encoder model for reranking",
    )
    device: str = Field(
        default="cpu",
        description="Device to run models on (cpu, cuda, mps)",
    )
    max_seq_length: int = Field(
        default=512,
        description="Maximum sequence length for tokenization",
    )
    batch_size: int = Field(default=32, description="Batch size for inference")


class ServerConfig(BaseModel):
    """Configuration for the ML worker server."""

    host: str = Field(default="127.0.0.1", description="Server bind host")
    http_port: int = Field(default=50052, description="HTTP API port")
    grpc_port: int = Field(default=50051, description="gRPC server port")
    models: ModelConfig = Field(default_factory=ModelConfig)
    lazy_load: bool = Field(
        default=True,
        description="Lazy-load models on first request instead of startup",
    )
