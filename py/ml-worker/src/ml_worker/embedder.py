"""Embedding service using Sentence Transformers."""

from __future__ import annotations

import logging
import time
from typing import TYPE_CHECKING

import numpy as np

if TYPE_CHECKING:
    from .config import ModelConfig

logger = logging.getLogger(__name__)


class Embedder:
    """Generates embeddings using Sentence Transformers models."""

    def __init__(self, config: ModelConfig) -> None:
        self._config = config
        self._model = None
        self._model_name: str | None = None

    def _ensure_loaded(self) -> None:
        """Lazy-load the model on first use."""
        if self._model is not None:
            return

        try:
            from sentence_transformers import SentenceTransformer

            logger.info("Loading embedding model: %s", self._config.embedding_model)
            self._model = SentenceTransformer(
                self._config.embedding_model,
                device=self._config.device,
            )
            self._model.max_seq_length = self._config.max_seq_length
            self._model_name = self._config.embedding_model
            logger.info("Embedding model loaded successfully")
        except ImportError:
            logger.warning(
                "sentence-transformers not installed, using random fallback embeddings"
            )
            self._model = None
            self._model_name = "fallback-random"

    def embed(self, texts: list[str]) -> EmbedResult:
        """Generate embeddings for a list of texts."""
        start = time.monotonic()
        self._ensure_loaded()

        if self._model is None:
            # Fallback: random embeddings for graceful degradation
            embeddings = np.random.default_rng(42).standard_normal((len(texts), 384)).astype(
                np.float32
            )
        else:
            embeddings = self._model.encode(
                texts,
                batch_size=self._config.batch_size,
                show_progress_bar=False,
                convert_to_numpy=True,
            )

        duration_ms = int((time.monotonic() - start) * 1000)

        return EmbedResult(
            embeddings=[e.tolist() for e in embeddings],
            model_name=self._model_name or "unknown",
            duration_ms=duration_ms,
            dimensions=embeddings.shape[1] if len(embeddings) > 0 else 0,
        )

    @property
    def is_loaded(self) -> bool:
        return self._model is not None or self._model_name == "fallback-random"


class EmbedResult:
    """Result of an embedding operation."""

    __slots__ = ("embeddings", "model_name", "duration_ms", "dimensions")

    def __init__(
        self,
        embeddings: list[list[float]],
        model_name: str,
        duration_ms: int,
        dimensions: int,
    ) -> None:
        self.embeddings = embeddings
        self.model_name = model_name
        self.duration_ms = duration_ms
        self.dimensions = dimensions
