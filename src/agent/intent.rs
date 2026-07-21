// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::agent::eval::{ComplexityTier, estimate_complexity};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IntentAnalysisConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_min_confidence")]
    pub min_confidence: f64,

    #[serde(default = "default_true")]
    pub enrich_preamble: bool,

    #[serde(default)]
    pub enforce_plan_threshold: bool,

    #[serde(default)]
    pub model: Option<String>,
}

fn default_min_confidence() -> f64 {
    0.6
}

fn default_true() -> bool {
    true
}

impl Default for IntentAnalysisConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_confidence: default_min_confidence(),
            enrich_preamble: default_true(),
            enforce_plan_threshold: false,
            model: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskIntent {
    Coding,
    Debug,
    Design,
    UiDesign,
    Plan,
    Qa,
    Curate,
    Tdd,
    General,
}

impl TaskIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Coding => "coding",
            Self::Debug => "debug",
            Self::Design => "design",
            Self::UiDesign => "ui_design",
            Self::Plan => "plan",
            Self::Qa => "qa",
            Self::Curate => "curate",
            Self::Tdd => "tdd",
            Self::General => "general",
        }
    }

    pub fn from_loose(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "coding" | "code" | "implement" => Self::Coding,
            "debug" | "bug" | "fix" | "troubleshoot" => Self::Debug,
            "architecture" | "architect" | "arch" => Self::Design,
            "ui_design" | "ui" | "designer" | "design" | "prototype" | "figma" => Self::UiDesign,
            "plan" | "planning" | "roadmap" => Self::Plan,
            "qa" | "question" | "ask" | "explain" => Self::Qa,
            "curate" | "curator" | "research" | "report" | "thesis" => Self::Curate,
            "tdd" | "test-driven" | "test_driven" => Self::Tdd,
            _ => Self::General,
        }
    }

    fn intent_note(self) -> Option<&'static str> {
        match self {
            Self::Coding => Some(
                "[Intent] The user is requesting a concrete code change or new functionality. \
                 Center this turn on that goal and act within the rules of the current coding mode.",
            ),
            Self::Debug => Some(
                "[Intent] The user is investigating or fixing a bug/failure. Prioritise \
                 understanding the root cause from evidence before acting, within the rules of the \
                 current coding mode.",
            ),
            Self::Design => Some(
                "[Intent] The user wants design/architecture reasoning. Weigh the approach and key \
                 trade-offs, within the rules of the current coding mode.",
            ),
            Self::UiDesign => Some(
                "[Intent] The user wants UI/visual design work. Prefer Designer-mode surfaces \
                 (prototype, dashboard, deck) and keep implementation secondary until the design \
                 artifact is approved.",
            ),
            Self::Plan => Some(
                "[Intent] The user is describing a multi-step task. Keep the work organised into \
                 clear ordered steps, within the rules of the current coding mode.",
            ),
            Self::Qa => Some(
                "[Intent] The user is asking a question and expects a clear, well-grounded \
                 explanation, within the rules of the current coding mode.",
            ),
            Self::Curate => Some(
                "[Intent] The user wants deep research and a written report/blueprint rather than \
                 immediate code changes. Prefer Curator-mode research → document delivery.",
            ),
            Self::Tdd => Some(
                "[Intent] The user wants test-driven delivery. Write a failing test first, then \
                 the minimum implementation to pass, then refactor.",
            ),
            Self::General => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IntentAnalysis {
    pub intent: TaskIntent,
    pub confidence: f64,
    pub complexity: ComplexityTier,
}

impl IntentAnalysis {
    pub fn is_confident(&self, min_confidence: f64) -> bool {
        self.confidence >= min_confidence
    }

    pub fn intent_note(&self) -> Option<&'static str> {
        self.intent.intent_note()
    }

    pub fn coding_mode(&self) -> crate::agent::coding_mode::CodingMode {
        use crate::agent::coding_mode::CodingMode;
        if self.confidence < 0.55 {
            return CodingMode::Agent;
        }
        match self.intent {
            TaskIntent::Debug => CodingMode::Debug,
            TaskIntent::Design => CodingMode::Architect,
            TaskIntent::UiDesign => CodingMode::Designer,
            TaskIntent::Plan => CodingMode::Plan,
            TaskIntent::Qa => CodingMode::Ask,
            TaskIntent::Curate => CodingMode::Curator,
            TaskIntent::Tdd => CodingMode::Tdd,
            TaskIntent::Coding | TaskIntent::General => CodingMode::Agent,
        }
    }
}

pub fn auto_select_coding_mode(message: &str) -> crate::agent::coding_mode::CodingMode {
    analyze_intent(message).coding_mode()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationIntent {
    Continue,
    Greeting,
    NewTask,
}

impl ConversationIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::Greeting => "greeting",
            Self::NewTask => "new_task",
        }
    }
}

const CONTINUE_MARKERS: &[&str] = &[
    "继续",
    "接着",
    "继续执行",
    "继续做",
    "接着做",
    "接着上",
    "繼續",
    "continue",
    "go on",
    "go ahead",
    "keep going",
    "carry on",
    "resume",
    "pick up where",
    "finish it",
    "finish the",
];

const GREETING_EXACT: &[&str] = &[
    "你好", "您好", "在吗", "在不在", "在么", "在", "嗨", "哈喽", "哈啰", "早", "早安",
    "早上好", "中午好", "下午好", "晚上好", "hi", "hello", "hey", "yo", "hiya", "嘿",
];

fn is_greeting(trimmed: &str) -> bool {
    let char_count = trimmed.chars().count();
    if char_count == 0 || char_count > 10 {
        return false;
    }
    let normalized = trimmed
        .trim_end_matches(['!', '！', '。', '.', '~', '～', '?', '？', ' ', ',', '，'])
        .to_lowercase();
    if GREETING_EXACT.iter().any(|g| normalized == *g) {
        return true;
    }
    normalized.starts_with("你好")
        || normalized.starts_with("您好")
        || normalized.starts_with("hello")
        || normalized.starts_with("hi ")
        || normalized.starts_with("hey ")
}

const CONTINUE_SUBSTRING_MAX_CHARS: usize = 24;

pub fn classify_conversation_intent(message: &str) -> ConversationIntent {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return ConversationIntent::NewTask;
    }
    let lower = trimmed.to_lowercase();
    let is_question = trimmed.ends_with('?') || trimmed.ends_with('？');
    let prefix_hit = CONTINUE_MARKERS.iter().any(|k| lower.starts_with(k));
    let short_hit = !is_question
        && trimmed.chars().count() <= CONTINUE_SUBSTRING_MAX_CHARS
        && CONTINUE_MARKERS.iter().any(|k| lower.contains(k));
    if prefix_hit || short_hit {
        return ConversationIntent::Continue;
    }
    if is_greeting(trimmed) {
        return ConversationIntent::Greeting;
    }
    ConversationIntent::NewTask
}

pub fn conversation_signal_note(
    intent: ConversationIntent,
    has_unfinished_task: bool,
) -> Option<&'static str> {
    match (intent, has_unfinished_task) {
        (ConversationIntent::Continue, true) => Some(
            "[Conversation signal] The latest message reads as an explicit request to continue. \
             Resume EXACTLY the single task recorded in [UNFINISHED EARLIER TASK] - that is the \
             MOST RECENT interrupted task - picking up from where it stopped per [CURRENT REQUEST] \
             and not redoing already-completed steps. Do NOT resume, restart, or merge in any \
             OLDER interrupted, unfinished, superseded, or [Recovered ...] task elsewhere in the \
             window or history, and do NOT invent a new task to continue. If [UNFINISHED EARLIER \
             TASK] does not match what the user means, ask briefly instead of guessing.",
        ),
        (ConversationIntent::Continue, false) => Some(
            "[Conversation signal] The latest message reads as a request to continue, but no \
             unfinished task is tracked. Use the conversation context to confirm what to continue; \
             if it is unclear, ask the user briefly instead of guessing.",
        ),
        (ConversationIntent::Greeting, _) => Some(
            "[Conversation signal] The latest message reads as a greeting / small talk. Greet back \
             and ask what they need; do NOT resume or start any earlier task on your own.",
        ),
        (ConversationIntent::NewTask, true) => Some(
            "[Conversation signal] The latest message reads as a new or unrelated request. Act on \
             [CURRENT REQUEST] directly; do NOT resume the tracked unfinished task unless this \
             message explicitly asks for it.",
        ),
        (ConversationIntent::NewTask, false) => None,
    }
}

const DEBUG_KEYWORDS: &[&str] = &[
    "bug", "error", "panic", "crash", "fail", "failing", "broken", "stack trace", "traceback",
    "exception", "not working", "doesn't work", "does not work", "regression", "调试", "报错",
    "崩溃", "异常", "修复",
];

const CODING_KEYWORDS: &[&str] = &[
    "implement", "add", "create", "write", "refactor", "rename", "build", "function", "class",
    "struct", "module", "method", "endpoint", "feature", "code", "实现", "添加", "新增", "重构",
    "编写", "修改",
];

const DESIGN_KEYWORDS: &[&str] = &[
    "architecture", "architect", "trade-off", "tradeoff", "system design", "架构", "系统设计",
    "技术方案",
];

const UI_DESIGN_KEYWORDS: &[&str] = &[
    "ui", "ux", "prototype", "figma", "dashboard", "mockup", "wireframe", "landing page",
    "界面", "原型", "视觉", "设计稿", "设计师", "slideshow", "slide deck",
];

const CURATOR_KEYWORDS: &[&str] = &[
    "curator", "research report", "technical report", "whitepaper", "thesis", "literature review",
    "调研", "论文", "研究报告", "技术方案文档", "docx",
];

const TDD_KEYWORDS: &[&str] = &[
    "tdd", "test-driven", "test driven", "red-green-refactor", "write a failing test",
    "测试驱动", "先写测试",
];

// Deliberately excludes weak connectives like "then"/"steps" that appear in
// ordinary coding requests ("fix the bug then run tests"): they used to push the
// keyword fallback into read-only Plan mode under Auto. Kept signals are ones
// that genuinely indicate a planning request.
const PLAN_KEYWORDS: &[&str] = &[
    "plan", "step by step", "roadmap", "milestone", "after that", "phase",
    "计划", "路线",
];

const QA_PREFIXES: &[&str] = &[
    "what", "why", "how", "when", "where", "who", "which", "is ", "are ", "does", "do ", "can ",
    "could", "should", "explain", "什么", "为什么", "怎么", "如何", "是否",
];

fn count_hits(lower: &str, keywords: &[&str]) -> usize {
    keywords.iter().filter(|kw| lower.contains(**kw)).count()
}

pub fn analyze_intent(message: &str) -> IntentAnalysis {
    let trimmed = message.trim();
    let lower = trimmed.to_lowercase();
    let complexity = estimate_complexity(trimmed);

    let debug_hits = count_hits(&lower, DEBUG_KEYWORDS);
    let coding_hits = count_hits(&lower, CODING_KEYWORDS);
    let design_hits = count_hits(&lower, DESIGN_KEYWORDS);
    let ui_hits = count_hits(&lower, UI_DESIGN_KEYWORDS);
    let curator_hits = count_hits(&lower, CURATOR_KEYWORDS);
    let tdd_hits = count_hits(&lower, TDD_KEYWORDS);
    let plan_hits = count_hits(&lower, PLAN_KEYWORDS);

    let is_question = trimmed.ends_with('?')
        || trimmed.ends_with('？')
        || QA_PREFIXES.iter().any(|p| lower.starts_with(p));

    let scored: [(TaskIntent, usize); 7] = [
        (TaskIntent::Debug, debug_hits),
        (TaskIntent::Coding, coding_hits),
        (TaskIntent::Design, design_hits),
        (TaskIntent::UiDesign, ui_hits),
        (TaskIntent::Curate, curator_hits),
        (TaskIntent::Tdd, tdd_hits),
        (TaskIntent::Plan, plan_hits),
    ];

    let best = scored
        .iter()
        .copied()
        .max_by_key(|(_, hits)| *hits)
        .filter(|(_, hits)| *hits > 0);

    // Require a strong plan signal (>=2 keyword hits) before overriding a coding
    // intent, so a single incidental planning word can't route a write task into
    // read-only Plan mode. A genuinely complex coding request with an explicit
    // plan word still qualifies.
    let plan_worthy = plan_hits >= 2
        || (matches!(complexity, ComplexityTier::Complex) && coding_hits > 0 && plan_hits > 0);

    let (intent, base) = match best {
        // Plan wins as the max only when its signal is strong; otherwise defer to
        // the next-best coding-ish intent on ties (array order put Plan last, so a
        // 1-1 tie previously handed the turn to Plan).
        Some((TaskIntent::Plan, hits)) if plan_worthy => (TaskIntent::Plan, hits),
        Some((TaskIntent::Plan, _)) => {
            let coding_like = [
                (TaskIntent::Debug, debug_hits),
                (TaskIntent::Coding, coding_hits),
                (TaskIntent::Tdd, tdd_hits),
                (TaskIntent::Design, design_hits),
                (TaskIntent::UiDesign, ui_hits),
                (TaskIntent::Curate, curator_hits),
            ]
            .into_iter()
            .filter(|(_, h)| *h > 0)
            .max_by_key(|(_, h)| *h);
            match coding_like {
                Some((intent, hits)) => (intent, hits),
                None if is_question => (TaskIntent::Qa, 1),
                None => (TaskIntent::General, 0),
            }
        }
        Some((_intent, hits)) if plan_worthy && plan_hits >= hits => (TaskIntent::Plan, plan_hits),
        Some((intent, hits)) => (intent, hits),
        None if is_question => (TaskIntent::Qa, 1),
        None => (TaskIntent::General, 0),
    };

    let mut confidence = match intent {
        TaskIntent::Qa => 0.65,
        TaskIntent::General => 0.3,
        _ => 0.5 + 0.15 * (base.min(3) as f64),
    };
    if matches!(complexity, ComplexityTier::Complex) {
        confidence += 0.05;
    }
    confidence = confidence.clamp(0.0, 1.0);

    IntentAnalysis {
        intent,
        confidence,
        complexity,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentDecision {
    Resume,
    NewTask,
    Greeting,
    Clarify,
}

impl IntentDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resume => "resume",
            Self::NewTask => "new_task",
            Self::Greeting => "greeting",
            Self::Clarify => "clarify",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LlmIntentDecision {
    pub decision: IntentDecision,
    pub resume_task_seq: Option<u64>,
    pub task_intent: TaskIntent,
    pub confidence: f64,
    pub reason: String,
}

impl LlmIntentDecision {
    pub fn coding_mode(&self) -> crate::agent::coding_mode::CodingMode {
        use crate::agent::coding_mode::CodingMode;
        if self.confidence < 0.55 {
            return CodingMode::Agent;
        }
        match self.task_intent {
            TaskIntent::Debug => CodingMode::Debug,
            TaskIntent::Design => CodingMode::Architect,
            TaskIntent::UiDesign => CodingMode::Designer,
            TaskIntent::Plan => CodingMode::Plan,
            TaskIntent::Qa => CodingMode::Ask,
            TaskIntent::Curate => CodingMode::Curator,
            TaskIntent::Tdd => CodingMode::Tdd,
            TaskIntent::Coding | TaskIntent::General => CodingMode::Agent,
        }
    }

    pub fn intent_note(&self) -> Option<&'static str> {
        self.task_intent.intent_note()
    }
}

pub const INTENT_SYSTEM_PROMPT: &str = "You are an intent classifier for a coding agent. \
You are given the user's CURRENT MESSAGE, the RECENT CONVERSATION (real turns only), and at most one \
CANDIDATE UNFINISHED TASK (the single most-recent task that was interrupted before finishing). \
Decide what the user wants RIGHT NOW and reply with ONLY one JSON object — no prose, no markdown, no code fences — \
with exactly these fields: \
{\"decision\":\"resume|new_task|greeting|clarify\",\"resume_task_seq\":<number or null>,\
\"task_intent\":\"coding|debug|design|ui_design|plan|qa|curate|tdd|general\",\"confidence\":<0.0-1.0>,\"reason\":\"<short>\"}. \
Rules: \
(1) \"resume\" ONLY when a CANDIDATE UNFINISHED TASK exists AND the current message clearly means to continue or \
finish THAT task (e.g. \"继续\"/\"continue\"/\"接着\"/\"go on\", or it explicitly refers to it); set resume_task_seq to that task's seq. \
(2) \"greeting\" for pure greeting / small talk with no actionable task. \
(3) \"clarify\" when the message is genuinely ambiguous and you cannot tell what to do or which task is meant. \
(4) \"new_task\" for anything else — act on the current message as a fresh request. \
The CANDIDATE UNFINISHED TASK is the ONLY resumable task; never treat any older or already-answered request as \
something to resume. Judge strictly from the provided context. \
task_intent describes the nature of the work: use ui_design for UI/visual/prototype work, design for software \
architecture trade-offs, curate for research reports/blueprints, tdd for test-driven delivery.";

const INTENT_MESSAGE_HEAD_CHARS: usize = 2_000;
const INTENT_MESSAGE_TAIL_CHARS: usize = 1_000;

fn bound_intent_message(message: &str) -> String {
    let trimmed = message.trim();
    let total = trimmed.chars().count();
    if total <= INTENT_MESSAGE_HEAD_CHARS + INTENT_MESSAGE_TAIL_CHARS {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(INTENT_MESSAGE_HEAD_CHARS).collect();
    let tail: String = trimmed
        .chars()
        .skip(total - INTENT_MESSAGE_TAIL_CHARS)
        .collect();
    let omitted = total - INTENT_MESSAGE_HEAD_CHARS - INTENT_MESSAGE_TAIL_CHARS;
    format!("{head}\n…[{omitted} chars omitted]…\n{tail}")
}

pub fn build_intent_user_prompt(
    current_message: &str,
    recent: &[(String, String)],
    candidate: Option<(u64, &str, &str)>,
) -> String {
    use std::fmt::Write as _;
    let mut prompt = String::new();
    let _ = write!(
        prompt,
        "[CURRENT MESSAGE]\n{}\n\n",
        bound_intent_message(current_message)
    );

    prompt.push_str("[RECENT CONVERSATION] (oldest first; may be empty)\n");
    if recent.is_empty() {
        prompt.push_str("(none)\n");
    } else {
        for (role, text) in recent {
            let _ = writeln!(prompt, "{role}: {text}");
        }
    }
    prompt.push('\n');

    prompt.push_str("[CANDIDATE UNFINISHED TASK] (may be none)\n");
    match candidate {
        Some((seq, request, digest)) => {
            let _ = writeln!(prompt, "seq={seq}");
            let _ = writeln!(prompt, "request: {}", request.trim());
            if !digest.trim().is_empty() {
                let _ = writeln!(prompt, "progress so far: {}", digest.trim());
            }
        }
        None => prompt.push_str("none\n"),
    }
    prompt.push_str("\nReturn ONLY the JSON object now.");
    prompt
}

fn extract_json_object(raw: &str) -> Option<String> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(raw[start..=end].to_string())
}

pub fn parse_llm_intent_decision(raw: &str) -> Option<LlmIntentDecision> {
    let json_str = extract_json_object(raw)?;
    let value: serde_json::Value = serde_json::from_str(&json_str).ok()?;

    let decision = match value
        .get("decision")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_ascii_lowercase())?
        .as_str()
    {
        "resume" => IntentDecision::Resume,
        "greeting" | "greet" => IntentDecision::Greeting,
        "clarify" | "ambiguous" => IntentDecision::Clarify,
        _ => IntentDecision::NewTask,
    };

    let resume_task_seq = value.get("resume_task_seq").and_then(|v| {
        if v.is_null() {
            None
        } else {
            v.as_u64().or_else(|| v.as_f64().map(|f| f as u64))
        }
    });

    let task_intent = value
        .get("task_intent")
        .and_then(|v| v.as_str())
        .map(TaskIntent::from_loose)
        .unwrap_or(TaskIntent::General);

    let confidence = value
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);

    let reason = value
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Some(LlmIntentDecision {
        decision,
        resume_task_seq,
        task_intent,
        confidence,
        reason,
    })
}

pub fn llm_conversation_signal_note(
    decision: &LlmIntentDecision,
    has_unfinished_task: bool,
) -> Option<&'static str> {
    match decision.decision {
        IntentDecision::Resume => {
            conversation_signal_note(ConversationIntent::Continue, has_unfinished_task)
        }
        IntentDecision::Greeting => {
            conversation_signal_note(ConversationIntent::Greeting, has_unfinished_task)
        }
        IntentDecision::NewTask => {
            conversation_signal_note(ConversationIntent::NewTask, has_unfinished_task)
        }
        IntentDecision::Clarify => Some(
            "[Conversation signal] The latest message is genuinely ambiguous. Briefly ask the user \
             to clarify what they want before acting; do NOT guess, and do NOT resume or start any \
             earlier task on your own.",
        ),
    }
}
