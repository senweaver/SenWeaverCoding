"""HTTP transport for the SenWeaverCoding Python SDK."""

import httpx
from typing import Any, Dict, Optional

from sen.errors import TransportError, from_code
from sen.transport.base import Transport


class HttpTransport(Transport):
    """JSON-RPC over HTTP transport.

    Supports basic authentication via the ``auth`` parameter, and respects
    the ``timeout`` setting for all requests.

    Example::

        async with HttpTransport("http://localhost:42618") as transport:
            result = await transport.call("system.info")
    """

    def __init__(
        self,
        base_url: str,
        auth: Optional[str] = None,
        timeout: float = 120.0,
    ) -> None:
        if not base_url.startswith("http://") and not base_url.startswith("https://"):
            raise ValueError(f"base_url must start with http:// or https://, got {base_url!r}")
        self.base_url = base_url.rstrip("/")
        self._client = httpx.AsyncClient(
            base_url=self.base_url,
            timeout=timeout,
            headers={"Content-Type": "application/json"},
        )
        if auth:
            self._client.headers["Authorization"] = f"Bearer {auth}"

    async def call(
        self,
        method: str,
        params: Optional[Dict[str, Any]] = None,
        request_id: Optional[str] = None,
    ) -> Any:
        payload: Dict[str, Any] = {
            "jsonrpc": "2.0",
            "method": method,
        }
        if params is not None:
            payload["params"] = params
        if request_id is not None:
            payload["id"] = request_id
        else:
            import uuid
            payload["id"] = str(uuid.uuid4())

        try:
            resp = await self._client.post("/rpc", json=payload)
        except httpx.ConnectError as e:
            raise TransportError(f"Connection failed to {self.base_url}: {e}") from e
        except httpx.TimeoutException as e:
            raise TransportError(f"Request timed out after {self._client.timeout}s: {e}") from e

        if resp.status_code != 200:
            raise TransportError(f"HTTP {resp.status_code}: {resp.text[:200]}")

        data = resp.json()
        if "error" in data:
            err = data["error"]
            raise from_code(err.get("code", -32000), err.get("message", "Unknown error"), err.get("data"))
        return data.get("result")

    async def close(self) -> None:
        await self._client.aclose()

    async def __aenter__(self) -> "HttpTransport":
        return self

    async def __aexit__(self, *_: Any) -> None:
        await self.close()
