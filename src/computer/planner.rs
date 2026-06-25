// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;

use super::action::{parse_planned_action, PlannedAction};
use super::vision::VisionClient;

const PLANNER_SYSTEM: &str = "You are an autonomous computer-use agent that controls a real \
desktop by looking at screenshots and deciding the single next action to take toward the \
user's goal.\n\n\
You can ONLY perform one action per step. After each action a fresh screenshot is taken so \
you can verify the result before the next step.\n\n\
Available actions:\n\
- click: click an element (provide element_description)\n\
- double_click: double-click an element (provide element_description)\n\
- right_click: right-click an element (provide element_description)\n\
- move_mouse: move the cursor over an element (provide element_description)\n\
- type: type text into the currently focused field (provide value)\n\
- key_press: press a key or shortcut (provide value, e.g. \"enter\", \"ctrl+c\", \"alt+tab\")\n\
- scroll: scroll the view (provide element_description for the area, value as one of up/down/left/right, optional amount)\n\
- drag: drag from one element to another (provide element_description and to_element_description)\n\
- wait: wait briefly for the UI to settle (optional amount in milliseconds)\n\
- finished: the task is complete (explain the result in thought)\n\
- call_user: you are blocked and need the human to intervene or provide info (explain in thought)\n\n\
Element descriptions must be concrete and visually unambiguous (text label, icon, position).\n\n\
If — and only if — you can confidently pinpoint the exact location yourself, you may add \
\"start_box\": [x, y] with coordinates normalized to 0-1000 (origin top-left); for drag also add \
\"end_box\": [x, y]. When you provide coordinates they are used directly; otherwise the \
element_description is used to locate the target. Prefer element_description unless you are sure.\n\n\
Respond with raw JSON only, no markdown, in this exact shape:\n\
{\"thought\": \"...\", \"action\": \"click\", \"element_description\": \"...\", \"value\": \"...\", \"amount\": 0, \"to_element_description\": \"...\", \"start_box\": [x, y], \"end_box\": [x, y]}\n\
Omit fields that do not apply.";

pub async fn plan_next(
    client: &VisionClient,
    image_data_uri: &str,
    task: &str,
    history: &[String],
) -> Result<PlannedAction> {
    let mut user = format!("User goal:\n{task}\n\n");
    if history.is_empty() {
        user.push_str("No actions have been taken yet. This is the first step.\n\n");
    } else {
        user.push_str("Actions taken so far:\n");
        for (idx, entry) in history.iter().enumerate() {
            user.push_str(&format!("{}. {entry}\n", idx + 1));
        }
        user.push('\n');
    }
    user.push_str(
        "Look at the current screenshot and decide the single next action. \
         Return JSON only.",
    );

    let raw = client
        .complete_with_image(PLANNER_SYSTEM, &user, image_data_uri)
        .await?;
    parse_planned_action(&raw)
}
