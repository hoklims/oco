"""HTTP API server for the ML worker using FastAPI."""

from __future__ import annotations

import logging
import sys
from contextlib import asynccontextmanager

from fastapi import FastAPI
from pydantic import BaseModel, Field

from .config import ServerConfig
from .embedder import Embedder
from .reranker import Reranker

logger = logging.getLogger(__name__)


class EmbedRequest(BaseModel):
    texts: list[str]
    model_name: str | None = None


class EmbedResponse(BaseModel):
    embeddings: list[list[float]]
    model_name: str
    duration_ms: int
    dimensions: int


class RerankRequest(BaseModel):
    query: str
    documents: list[str]
    top_k: int = Field(default=10, ge=1, le=100)


class RerankScoreResponse(BaseModel):
    index: int
    score: float
    document: str


class RerankResponse(BaseModel):
    scores: list[RerankScoreResponse]
    model_name: str
    duration_ms: int


class HealthResponse(BaseModel):
    status: str
    embedder_loaded: bool
    reranker_loaded: bool
    version: str


# Global state
_config = ServerConfig()
_embedder: Embedder | None = None
_reranker: Reranker | None = None


@asynccontextmanager
async def lifespan(app: FastAPI):  # noqa: ARG001
    global _embedder, _reranker  # noqa: PLW0603
    _embedder = Embedder(_config.models)
    _reranker = Reranker(_config.models)

    if not _config.lazy_load:
        logger.info("Pre-loading models...")
        _embedder.embed(["warmup"])
        _reranker.rerank("warmup", ["warmup"])
        logger.info("Models pre-loaded")

    yield


app = FastAPI(
    title="OCO ML Worker",
    version="0.1.0",
    description="Embedding and reranking service for Open Context Orchestrator",
    lifespan=lifespan,
)


@app.get("/health", response_model=HealthResponse)
async def health():
    return HealthResponse(
        status="ok",
        embedder_loaded=_embedder.is_loaded if _embedder else False,
        reranker_loaded=_reranker.is_loaded if _reranker else False,
        version="0.1.0",
    )


@app.post("/embed", response_model=EmbedResponse)
async def embed(request: EmbedRequest):
    assert _embedder is not None  # noqa: S101
    result = _embedder.embed(request.texts)
    return EmbedResponse(
        embeddings=result.embeddings,
        model_name=result.model_name,
        duration_ms=result.duration_ms,
        dimensions=result.dimensions,
    )


@app.post("/rerank", response_model=RerankResponse)
async def rerank(request: RerankRequest):
    assert _reranker is not None  # noqa: S101
    result = _reranker.rerank(request.query, request.documents, request.top_k)
    return RerankResponse(
        scores=[
            RerankScoreResponse(index=s.index, score=s.score, document=s.document)
            for s in result.scores
        ],
        model_name=result.model_name,
        duration_ms=result.duration_ms,
    )


def main():
    """Entry point for the ML worker server."""
    import uvicorn

    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )

    logger.info("Starting OCO ML Worker on %s:%d", _config.host, _config.http_port)
    uvicorn.run(
        "ml_worker.server:app",
        host=_config.host,
        port=_config.http_port,
        log_level="info",
    )


if __name__ == "__main__":
    main()
