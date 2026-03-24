"""Tests for the reranker service."""

from ml_worker.config import ModelConfig
from ml_worker.reranker import Reranker


def test_fallback_reranker():
    """Test keyword overlap reranking."""
    config = ModelConfig()
    reranker = Reranker(config)
    reranker._model = None
    reranker._model_name = "fallback-length"

    result = reranker.rerank(
        query="authentication token refresh",
        documents=[
            "The auth token is refreshed every hour",
            "Database migration guide",
            "Token refresh logic in auth module",
        ],
        top_k=2,
    )

    assert len(result.scores) == 2
    # The documents with "token" and "refresh" should score higher
    assert result.scores[0].score > 0


def test_rerank_empty_documents():
    """Test reranking with empty document list."""
    config = ModelConfig()
    reranker = Reranker(config)
    reranker._model = None
    reranker._model_name = "fallback-length"

    result = reranker.rerank(query="test", documents=[], top_k=5)
    assert len(result.scores) == 0
