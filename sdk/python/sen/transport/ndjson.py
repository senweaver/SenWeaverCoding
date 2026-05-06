"""NDJSON transport — spawns ``sen agent --output-format stream-json``
and communicates over stdin/stdout using newline-delimited JSON."""

from __future__ import annotations

import asyncio
import json
import logging
from typing import Any, Callable, Optional

from sen.ndjson_models import (
    ControlRequest,
    ControlResponse,
    StdinMessage,
    StdoutMessage,
    parse_stdout_message,
)

log = logging.getLogger(__name__)


class NdjsonTransport:
    """Async NDJSON transport over a ``sen`` subprocess.

    This is a standalone transport that speaks the streaming NDJSON protocol
    (``--output-format stream-json``), **not** JSON-RPC.  It does not inherit
    from :class:`~sen.transport.base.Transport`.

    Usage::

        async with NdjsonTransport(binary="sen") as t:
            await t.send(UserMessage(content="Hello"))
            msg = await t.recv()
    """

    def __init__(
        self,
        binary: str = "sen",
        args: Optional[list[str]] = None,
        cwd: Optional[str] = None,
        on_permission: Optional[Callable[..., Any]] = None,
        env: Optional[dict[str, str]] = None,
    ) -> None:
        self._binary = binary
        self._extra_args: list[str] = args or []
        self._cwd = cwd
        self._on_permission = on_permission
        self._env = env

        self._proc: Optional[asyncio.subprocess.Process] = None
        self._queue: asyncio.Queue[StdoutMessage] = asyncio.Queue()
        self._read_task: Optional[asyncio.Task[None]] = None
        self._closed = False

    # ── lifecycle ─────────────────────────────────────────────────────

    async def start(self) -> None:
        """Spawn the subprocess and begin reading stdout."""
        cmd = [
            self._binary,
            "agent",
            "--output-format",
            "stream-json",
            *self._extra_args,
        ]
        self._proc = await asyncio.create_subprocess_exec(
            *cmd,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            cwd=self._cwd,
            env=self._env,
        )
        self._read_task = asyncio.create_task(self._read_loop())

    async def close(self) -> None:
        """Terminate the subprocess and cancel the reader."""
        if self._closed:
            return
        self._closed = True

        if self._read_task and not self._read_task.done():
            self._read_task.cancel()
            try:
                await self._read_task
            except asyncio.CancelledError:
                pass

        if self._proc:
            try:
                self._proc.terminate()
                await asyncio.wait_for(self._proc.wait(), timeout=5)
            except (ProcessLookupError, asyncio.TimeoutError):
                self._proc.kill()
            self._proc = None

    async def __aenter__(self) -> NdjsonTransport:
        await self.start()
        return self

    async def __aexit__(self, *_: Any) -> None:
        await self.close()

    # ── send / recv ───────────────────────────────────────────────────

    async def send(self, msg: StdinMessage) -> None:
        """Write a :class:`StdinMessage` as a single NDJSON line to stdin."""
        if self._proc is None or self._proc.stdin is None:
            raise RuntimeError("Transport not started")

        raw = json.dumps(msg.to_dict(), ensure_ascii=False)
        raw = raw.replace("\u2028", "\\u2028").replace("\u2029", "\\u2029")
        self._proc.stdin.write((raw + "\n").encode("utf-8"))
        await self._proc.stdin.drain()

    async def recv(self) -> StdoutMessage:
        """Return the next parsed :class:`StdoutMessage` from the queue.

        Blocks until a message is available.  ``control_request`` messages
        are handled internally by the read loop and never appear here unless
        no ``on_permission`` callback was provided.
        """
        return await self._queue.get()

    # ── internal read loop ────────────────────────────────────────────

    async def _read_loop(self) -> None:
        assert self._proc is not None and self._proc.stdout is not None
        try:
            while True:
                line = await self._proc.stdout.readline()
                if not line:
                    break
                line_str = line.decode("utf-8", errors="replace").strip()
                if not line_str:
                    continue
                try:
                    data = json.loads(line_str)
                except json.JSONDecodeError:
                    log.warning("Ignoring non-JSON line: %s", line_str[:120])
                    continue

                msg = parse_stdout_message(data)

                if isinstance(msg, ControlRequest) and self._on_permission is not None:
                    await self._handle_control_request(msg)
                else:
                    await self._queue.put(msg)
        except asyncio.CancelledError:
            return
        except Exception:
            log.exception("NdjsonTransport read loop crashed")

    async def _handle_control_request(self, req: ControlRequest) -> None:
        """Invoke the ``on_permission`` callback and send the response."""
        assert self._on_permission is not None
        try:
            result = self._on_permission(req)
            if asyncio.iscoroutine(result):
                result = await result

            if isinstance(result, ControlResponse):
                response = result
            elif result is True or result is None:
                response = ControlResponse(
                    request_id=req.request_id, decision="allow"
                )
            elif result is False:
                response = ControlResponse(
                    request_id=req.request_id,
                    decision="deny",
                    reason="denied by SDK callback",
                )
            else:
                response = ControlResponse(
                    request_id=req.request_id, decision="allow"
                )
        except Exception as exc:
            log.error("on_permission callback raised: %s — denying by default", exc)
            response = ControlResponse(
                request_id=req.request_id, decision="deny"
            )

        await self.send(response)
