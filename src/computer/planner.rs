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
Before choosing an action, FIRST verify against the current screenshot whether the goal is \
already satisfied. If the goal is already achieved (for example the target app/window is \
already open and focused, or the requested state is already visible), respond with the \
\"finished\" action immediately and explain what you observe in thought. Never repeat an \
action whose intended result is already visible on screen.\n\n\
Look at the action history: if your previous action already produced its intended effect \
(the app opened, the dialog appeared, the text was entered), move on to the next step or \
finish - do NOT click the same element again. If the previous action clearly had no effect \
after a fresh screenshot, the click likely missed; re-locate the target more precisely or \
try a nearby, more specific description rather than repeating the exact same action.\n\n\
Prefer a single \"click\" (or double_click) directly on the target - it moves the cursor and \
clicks in one atomic step. Do NOT emit a separate \"move_mouse\" before a click; only use \
move_mouse when the goal is purely to hover.\n\n\
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
A small floating status card belonging to this assistant may be visible on the screen (usually \
near a corner, showing a round orb, a brand name and a status line). It is not part of the task: \
ignore it completely, never click or drag it, and never treat it as a target or as an obstruction \
to clear.\n\n\
The user can inject live instructions while you work; they appear in the action history as \
entries starting with \"USER UPDATE\". The most recent USER UPDATE always has priority. Decide \
for yourself whether it refines the current goal, changes how you should proceed, or replaces \
the goal with a completely new task - and briefly acknowledge that decision in your thought. \
If it is a new task, abandon the previous goal and work toward the new one immediately.\n\n\
Besides the current screenshot, the user may attach extra reference images (mockups, target \
states, documents). The FIRST image is always the live screenshot of the screen you control; \
any additional images are user-provided references, never the live screen.\n\n\
Coordinates: for every action that targets an element (click, double_click, right_click, \
move_mouse, scroll, drag) you SHOULD provide \"start_box\": [x, y] with the target point \
normalized to 0-1000 (origin top-left); for drag also provide \"end_box\": [x, y] for the \
destination. Look carefully at the screenshot and estimate the center of the target as \
precisely as you can - providing coordinates lets the action run in a single step. ONLY omit \
start_box when you genuinely cannot estimate the position; in that case provide a precise \
element_description and it will be located separately. Always also include element_description \
alongside coordinates when possible, as a fallback.\n\n\
Respond with raw JSON only, no markdown, in this exact shape:\n\
{\"thought\": \"...\", \"action\": \"click\", \"element_description\": \"...\", \"value\": \"...\", \"amount\": 0, \"to_element_description\": \"...\", \"start_box\": [x, y], \"end_box\": [x, y], \"display\": 0}\n\
Omit fields that do not apply.";

const MAX_HISTORY_ENTRIES: usize = 40;

pub async fn plan_next(
    client: &VisionClient,
    image_data_uri: &str,
    task: &str,
    history: &[String],
    reference_images: &[&str],
    display_hint: Option<&str>,
) -> Result<PlannedAction> {
    let mut user = format!("User goal:\n{task}\n\n");
    if let Some(hint) = display_hint {
        user.push_str(hint);
        user.push_str("\n\n");
    }
    if history.is_empty() {
        user.push_str("No actions have been taken yet. This is the first step.\n\n");
    } else {
        let skipped = history.len().saturating_sub(MAX_HISTORY_ENTRIES);
        if skipped > 0 {
            user.push_str(&format!(
                "Actions taken so far ({skipped} earlier actions omitted):\n"
            ));
        } else {
            user.push_str("Actions taken so far:\n");
        }
        for (idx, entry) in history.iter().enumerate().skip(skipped) {
            user.push_str(&format!("{}. {entry}\n", idx + 1));
        }
        user.push('\n');
    }
    if !reference_images.is_empty() {
        user.push_str(&format!(
            "The user attached {} reference image(s); they follow the live screenshot.\n\n",
            reference_images.len()
        ));
    }
    user.push_str(
        "Look at the current screenshot and decide the single next action. \
         Return JSON only.",
    );

    if reference_images.is_empty() {
        let raw = client
            .complete_with_image(PLANNER_SYSTEM, &user, image_data_uri)
            .await?;
        return parse_planned_action(&raw);
    }
    let mut uris: Vec<&str> = Vec::with_capacity(reference_images.len() + 1);
    uris.push(image_data_uri);
    uris.extend_from_slice(reference_images);
    let raw = client
        .complete_with_images(PLANNER_SYSTEM, &user, &uris)
        .await?;
    parse_planned_action(&raw)
}
