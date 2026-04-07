"""Base transport interface for the SenWeaverCoding Python SDK."""

from abc import ABC, abstractmethod
from typing import Any, Dict, Optional


class Transport(ABC):
    """Abstract base class for RPC transports."""

    @abstractmethod
    async def call(self, method: str, params: Optional[Dict[str, Any]] = None) -> Any:
        """Send a JSON-RPC request and return the result."""
        ...

    @abstractmethod
    async def close(self) -> None:
        """Close the transport."""
        ...
