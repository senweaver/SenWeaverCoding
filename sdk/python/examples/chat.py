"""Example: basic chat session with SenWeaverCoding."""

import asyncio

from sen import SenAgent


async def main() -> None:
    async with SenAgent(http_url="http://localhost:42618") as client:
        print("=== SenWeaverCoding Chat ===")

        # System health check
        health = await client.health()
        print(f"System: {health.status} | v{health.version} | {health.active_sessions} session(s)\n")

        # Create a session
        session = await client.create_session()
        print(f"Session created: {session.id}\n")

        # Chat
        messages = [
            "Hello! Who are you?",
            "What tools do you have available?",
            "Store a note that I prefer concise answers.",
            "What tools are available to me?",
        ]

        for msg in messages:
            print(f"You: {msg}")
            response = await session.prompt(msg)
            print(f"Agent: {response}\n")

        # Memory
        memories = await session.memory_recall("note")
        print(f"Recall results: {memories}")

        # Cleanup
        await session.kill()
        print("Session terminated.")


if __name__ == "__main__":
    asyncio.run(main())
