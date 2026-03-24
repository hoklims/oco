"""Tests for the embedder service."""

from ml_worker.config import ModelConfig
from ml_worker.embedder import Embedder


def test_fallback_embedder():
    """Test that the fallback embedder produces valid output shape."""
    config = ModelConfig(embedding_model="nonexistent-model")
    embedder = Embedder(config)
    # Force fallback by not installing sentence-transformers in test env
    embedder._model = None
    embedder._model_name = "fallback-random"

    result = embedder.embed(["hello world", "test query"])

    assert len(result.embeddings) == 2
    assert result.model_name == "fallback-random"
    assert result.duration_ms >= 0


def test_embed_empty_list():
    """Test embedding an empty list."""
    config = ModelConfig()
    embedder = Embedder(config)
    embedder._model = None
    embedder._model_name = "fallback-random"

    result = embedder.embed([])
    assert len(result.embeddings) == 0
