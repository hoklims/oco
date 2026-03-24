"""Reranking service using cross-encoder models."""

from __future__ import annotations

import logging
import time
from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .config import ModelConfig

logger = logging.getLogger(__name__)


@dataclass
class RerankScore:
    """Score for a single document in reranking."""

    index: int
    score: float
    document: str


@dataclass
class RerankResult:
    """Result of a reranking operation."""

    scores: list[RerankScore]
    model_name: str
    duration_ms: int


class Reranker:
    """Reranks documents using a cross-encoder model."""

    def __init__(self, config: ModelConfig) -> None:
        self._config = config
        self._model = None
        self._model_name: str | None = None

    def _ensure_loaded(self) -> None:
        if self._model is not None:
            return

        try:
            from sentence_transformers import CrossEncoder

            logger.info("Loading reranker model: %s", self._config.reranker_model)
            self._model = CrossEncoder(
                self._config.reranker_model,
                device=self._config.device,
            )
            self._model_name = self._config.reranker_model
            logger.info("Reranker model loaded successfully")
        except ImportError:
            logger.warning(
                "sentence-transformers not installed, using length-based fallback reranking"
            )
            self._model = None
            self._model_name = "fallback-length"

    def rerank(self, query: str, documents: list[str], top_k: int = 10) -> RerankResult:
        """Rerank documents by relevance to the query."""
        start = time.monotonic()
        self._ensure_loaded()

        if self._model is None:
            # Fallback: score by keyword overlap
            scores = self._fallback_rerank(query, documents)
        else:
            pairs = [(query, doc) for doc in documents]
            raw_scores = self._model.predict(pairs, batch_size=self._config.batch_size)
            scores = [
                RerankScore(index=i, score=float(s), document=documents[i])
                for i, s in enumerate(raw_scores)
            ]

        # Sort by score descending, take top_k
        scores.sort(key=lambda x: x.score, reverse=True)
        scores = scores[:top_k]

        duration_ms = int((time.monotonic() - start) * 1000)

        return RerankResult(
            scores=scores,
            model_name=self._model_name or "unknown",
            duration_ms=duration_ms,
        )

    def _fallback_rerank(self, query: str, documents: list[str]) -> list[RerankScore]:
        """Simple keyword overlap reranking for graceful degradation."""
        query_terms = set(query.lower().split())
        scores = []
        for i, doc in enumerate(documents):
            doc_terms = set(doc.lower().split())
            overlap = len(query_terms & doc_terms)
            score = overlap / max(len(query_terms), 1)
            scores.append(RerankScore(index=i, score=score, document=doc))
        return scores

    @property
    def is_loaded(self) -> bool:
        return self._model is not None or self._model_name == "fallback-length"
