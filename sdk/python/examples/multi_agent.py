"""Example: multi-agent coordination via blackboard."""

import asyncio
import uuid

from sen import SenAgent


async def main() -> None:
    """Simulate two agents coordinating via the shared blackboard.

    Agent Alpha writes tasks, Agent Beta picks them up.
    """
    async with SenAgent(http_url="http://localhost:42618") as client:
        # Create two isolated sessions
        alpha = await client.create_session()
        beta = await client.create_session()

        print(f"Alpha session: {alpha.id}")
        print(f"Beta session:  {beta.id}\n")

        # Alpha posts a task to the blackboard
        task_id = str(uuid.uuid4())
        task = {
            "id": task_id,
            "type": "analysis",
            "prompt": "What is the current date and time?",
            "status": "pending",
        }
        await alpha.blackboard_put(f"task:{task_id}", task, namespace="tasks")
        print(f"Alpha posted task {task_id}: {task['prompt']}")

        # Beta polls and picks up the task
        keys = await beta.blackboard_list(namespace="tasks")
        print(f"Beta sees tasks: {keys}")

        for key in keys:
            task = await beta.blackboard_get(key, namespace="tasks")
            if task and task.get("status") == "pending":
                print(f"Beta picked up: {task['prompt']}")
                result = await beta.prompt(task["prompt"])

                # Update blackboard with result
                task["status"] = "done"
                task["result"] = result
                await beta.blackboard_put(f"task:{task['id']}", task, namespace="tasks")
                print(f"Beta completed: {result[:80]}...")

        # Alpha checks the result
        completed_task = await alpha.blackboard_get(f"task:{task_id}", namespace="tasks")
        print(f"\nAlpha sees result: {completed_task['status']} — {completed_task.get('result', '')[:60]}...")

        # Cleanup
        await alpha.kill()
        await beta.kill()


if __name__ == "__main__":
    asyncio.run(main())
