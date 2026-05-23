// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::state::CuratorTemplateKind;

pub struct TemplateInfo {
    pub kind: CuratorTemplateKind,
    pub display_name: &'static str,
    pub description: &'static str,
    pub draft_markdown: &'static str,
    pub blueprint_markdown: &'static str,
}

pub fn all_templates() -> &'static [TemplateInfo] {
    static TEMPLATES: [TemplateInfo; 14] = [
        TemplateInfo {
            kind: CuratorTemplateKind::PaperImrad,
            display_name: "Academic Paper (IMRaD, generic)",
            description: "Generic IMRaD research paper skeleton. Use when no specific citation style is mandated.",
            draft_markdown: PAPER_IMRAD_DRAFT,
            blueprint_markdown: PAPER_BLUEPRINT,
        },
        TemplateInfo {
            kind: CuratorTemplateKind::PaperApa,
            display_name: "Academic Paper (APA 7)",
            description: "APA 7th Edition — Times New Roman 12pt, double-spaced, author-date in-text citation, References list with hanging indent. For psychology / education / social sciences.",
            draft_markdown: PAPER_APA_DRAFT,
            blueprint_markdown: PAPER_BLUEPRINT,
        },
        TemplateInfo {
            kind: CuratorTemplateKind::PaperMla,
            display_name: "Academic Paper (MLA 9)",
            description: "MLA 9th Edition — Times New Roman 12pt, double-spaced, author-page in-text citation, Works Cited list. For language / literature / humanities.",
            draft_markdown: PAPER_MLA_DRAFT,
            blueprint_markdown: PAPER_BLUEPRINT,
        },
        TemplateInfo {
            kind: CuratorTemplateKind::PaperChicago,
            display_name: "Academic Paper (Chicago 17/18)",
            description: "Chicago Manual of Style 17th/18th — 12pt double-spaced, notes-bibliography or author-date system, Bibliography list. For history / arts / publishing.",
            draft_markdown: PAPER_CHICAGO_DRAFT,
            blueprint_markdown: PAPER_BLUEPRINT,
        },
        TemplateInfo {
            kind: CuratorTemplateKind::PaperGb7714,
            display_name: "学术论文 (GB/T 7714-2015 / 2025-ready)",
            description: "中国国家标准 GB/T 7714 — 宋体小四正文，1.5 倍行距，参考文献按 GB/T 7714 著录（兼容 2015 与 2026 年生效的 2025 版，新增预印本/数据集著录规则）。",
            draft_markdown: PAPER_GB7714_DRAFT,
            blueprint_markdown: PAPER_BLUEPRINT,
        },
        TemplateInfo {
            kind: CuratorTemplateKind::SolutionFunctional,
            display_name: "工程解决方案（功能设计为核心，中文标准样式）",
            description: "面向工程落地的解决方案模板，重点在「功能设计」：总体架构 → 核心功能模块（每个模块包含：功能描述/技术原理/关键指标/数据与接口/实现要点）→ 配置环境 → 部署运维 → 验收与监控。表格按真实 DOCX 表格渲染。",
            draft_markdown: SOLUTION_FUNCTIONAL_DRAFT,
            blueprint_markdown: SOLUTION_FUNCTIONAL_BLUEPRINT,
        },
        TemplateInfo {
            kind: CuratorTemplateKind::SolutionGb8567_2006,
            display_name: "软件解决方案 (GB/T 8567-2006)",
            description: "中国国家标准 GB/T 8567-2006《计算机软件文档编制规范》，涵盖软件生存周期 25 种主要文档；本模板按招投标/验收文档结构编排正文章节。",
            draft_markdown: SOLUTION_GB8567_2006_DRAFT,
            blueprint_markdown: SOLUTION_FUNCTIONAL_BLUEPRINT,
        },
        TemplateInfo {
            kind: CuratorTemplateKind::SolutionGb8567_1988,
            display_name: "软件解决方案 (GB/T 8567-1988)",
            description: "GB/T 8567-1988（13 种核心文档）历史规范模板；用于沿用旧标准的项目。",
            draft_markdown: SOLUTION_GB8567_1988_DRAFT,
            blueprint_markdown: SOLUTION_FUNCTIONAL_BLUEPRINT,
        },
        TemplateInfo {
            kind: CuratorTemplateKind::SolutionIeee830,
            display_name: "Software Requirements Specification (IEEE 830-1998)",
            description: "IEEE 830-1998 SRS — 经典软件需求规格说明书模板（Introduction / Overall Description / Specific Requirements / Appendices）。",
            draft_markdown: SOLUTION_IEEE830_DRAFT,
            blueprint_markdown: SOLUTION_FUNCTIONAL_BLUEPRINT,
        },
        TemplateInfo {
            kind: CuratorTemplateKind::SolutionIso29148,
            display_name: "Software Requirements Specification (ISO/IEC/IEEE 29148:2011)",
            description: "现代 SRS 国际标准（取代并扩展 IEEE 830）；涵盖 system context / stakeholder needs / system requirements / verification / supporting information。",
            draft_markdown: SOLUTION_ISO29148_DRAFT,
            blueprint_markdown: SOLUTION_FUNCTIONAL_BLUEPRINT,
        },
        TemplateInfo {
            kind: CuratorTemplateKind::SolutionIso42010,
            display_name: "Software Architecture Description (ISO/IEC/IEEE 42010)",
            description: "软件架构描述权威国际标准（42010）；以 Stakeholders / Concerns / Viewpoints / Views / Correspondence Rules 为骨架，多重视图描绘系统。",
            draft_markdown: SOLUTION_ISO42010_DRAFT,
            blueprint_markdown: SOLUTION_FUNCTIONAL_BLUEPRINT,
        },
        TemplateInfo {
            kind: CuratorTemplateKind::SolutionIeee1016,
            display_name: "Software Design Description (IEEE 1016-2009)",
            description: "IEEE 1016-2009 SDD — 详细设计文档规范（Design Identification / Stakeholders & Concerns / Views / Rationale）。",
            draft_markdown: SOLUTION_IEEE1016_DRAFT,
            blueprint_markdown: SOLUTION_FUNCTIONAL_BLUEPRINT,
        },
        TemplateInfo {
            kind: CuratorTemplateKind::SolutionIso12207,
            display_name: "Software Lifecycle Process (ISO/IEC/IEEE 12207)",
            description: "软件生命周期过程通用框架；按 Agreement / Organizational / Project / Technical 过程组织文档结构。",
            draft_markdown: SOLUTION_ISO12207_DRAFT,
            blueprint_markdown: SOLUTION_FUNCTIONAL_BLUEPRINT,
        },
        TemplateInfo {
            kind: CuratorTemplateKind::TechReport,
            display_name: "Technical Report",
            description: "Engineering technical report (Abstract / Background / Objectives / Methodology / Results / Analysis / Recommendations / References / Appendix).",
            draft_markdown: TECH_REPORT_DRAFT,
            blueprint_markdown: TECH_REPORT_BLUEPRINT,
        },
    ];
    &TEMPLATES
}

pub fn template_for(kind: CuratorTemplateKind) -> &'static TemplateInfo {
    all_templates()
        .iter()
        .find(|t| t.kind == kind)
        .unwrap_or(&all_templates()[0])
}

pub fn list_summary() -> String {
    let mut out = String::new();
    out.push_str("# Curator Templates\n\n");
    for tpl in all_templates() {
        out.push_str(&format!(
            "- `{}` — {}\n  {}\n",
            tpl.kind.label(),
            tpl.display_name,
            tpl.description
        ));
    }
    out
}

// ============================================================================
// PAPER TEMPLATES
// ============================================================================

const PAPER_IMRAD_DRAFT: &str = "# <Title>\n\n\
> Curator template: **Academic Paper — IMRaD (generic)**\n\n\
**Authors**: <name1>, <name2>  \n\
**Affiliation**: <institution>  \n\
**Keywords**: <kw1>, <kw2>, <kw3>\n\n\
## Abstract\n\n\
<150–250 words: context → problem → method → key result → contribution.>\n\n\
## 1. Introduction\n\n\
<Motivation, problem statement, research questions, paper structure. Cite background via `[Sn]`.>\n\n\
## 2. Related Work\n\n\
<Group prior work; state the gap this paper closes.>\n\n\
## 3. Method\n\n\
<Algorithm / system / theory. Include precise notation, pseudo-code, diagrams. Reference workspace artefacts via `path:lineStart-lineEnd`.>\n\n\
## 4. Experiments / Evaluation\n\n\
<Datasets, baselines, metrics, hardware, ablation. Use Markdown tables — they will be rendered as real DOCX tables.>\n\n\
| Setting | Baseline | Proposed | Δ |\n\
|---------|----------|----------|---|\n\
| <…>     | <…>      | <…>      | <…> |\n\n\
## 5. Discussion\n\n\
<Interpret results; threats to validity; failure modes.>\n\n\
## 6. Conclusion\n\n\
<Recap contribution; outline future work.>\n\n\
## References\n\n\
<Imported from `sources.md`. Each entry: `[Sn] Authors. Title. Venue, Year. URL.`>\n";

const PAPER_APA_DRAFT: &str = "# <Title in Title Case>\n\n\
> Curator template: **Academic Paper — APA 7th Edition**  \n\
> Formatting: Times New Roman 12pt, double-spaced, 1 inch margins, page header with running head and page number.\n\n\
**Authors**: <First Last>, <First Last>  \n\
**Affiliation**: <Department, Institution>  \n\
**Author Note**: <conflicts of interest, funding, contact email>\n\n\
## Abstract\n\n\
<150–250 words. Plain-text paragraph (no indent), labelled \"Abstract\" centred. Followed by `Keywords:` line.>\n\n\
*Keywords*: <kw1>, <kw2>, <kw3>\n\n\
## Introduction\n\n\
<Begin on a new page. Title centred, bold. Lead with the problem, situate it in the literature, state the present study. In-text citations use the **author–date** form: (Smith, 2023, p. 45) or Smith (2023) argued …>\n\n\
## Method\n\n\
### Participants\n<demographics, sampling>\n\n\
### Materials\n<instruments, stimuli>\n\n\
### Procedure\n<step-by-step protocol>\n\n\
### Data Analysis Plan\n<analytic approach>\n\n\
## Results\n\n\
<Report descriptive then inferential. Statistical reporting follows APA 7 (italic statistic, df in parentheses, exact p where possible). Tables and figures use APA captions.>\n\n\
| Variable | M | SD | n |\n\
|----------|---|----|---|\n\
| <…>      | <…> | <…> | <…> |\n\n\
## Discussion\n\n\
<Interpret findings, limitations, future directions, theoretical and practical implications.>\n\n\
## References\n\n\
<APA 7 References section starts on a new page, title \"References\" centred & bold. Entries alphabetical by first author; hanging indent 0.5\". Examples:\n\
- Smith, J. A., & Lee, K. (2023). *Title of work*. Publisher. https://doi.org/10.xxxx/yyyy\n\
- Jones, R. (2024). Article title. *Journal Name, 12*(3), 45–67. https://doi.org/10.xxxx/zzzz\n\
Cross-reference every in-text `(Author, Year)` with a `[Sn]` entry in `sources.md`.>\n";

const PAPER_MLA_DRAFT: &str = "# <Title in Title Case>\n\n\
> Curator template: **Academic Paper — MLA 9th Edition**  \n\
> Formatting: Times New Roman 12pt, double-spaced, 1 inch margins, header `Surname Page#` top-right of every page.\n\n\
**Author**: <First Last>  \n\
**Course / Instructor**: <…>  \n\
**Date**: <DD Month YYYY>\n\n\
## Introduction\n\n\
<Open with the thesis. MLA uses **author-page** in-text citations: (Smith 45) or Smith argues that ... (45).>\n\n\
## Body — Argument 1\n\n\
<Topic sentence; evidence with parenthetical citation; analysis; transition.>\n\n\
## Body — Argument 2\n\n\
<…>\n\n\
## Body — Counterargument & Response\n\n\
<…>\n\n\
## Conclusion\n\n\
<Synthesise; restate significance; broader implications.>\n\n\
## Works Cited\n\n\
<MLA 9 \"Works Cited\" page; entries alphabetical by author surname; hanging indent. Core elements:\n\
Author. \"Title of Source.\" *Title of Container*, Other Contributors, Version, Number, Publisher, Publication Date, Location.\n\n\
Examples:\n\
- Smith, John. *Title of Book*. Publisher, 2023.\n\
- Lee, Karen. \"Article Title.\" *Journal Name*, vol. 12, no. 3, 2024, pp. 45–67. *JSTOR*, https://www.jstor.org/stable/xxxx.\n\n\
Every parenthetical `(Author Page)` must map back to a `[Sn]` entry in `sources.md`.>\n";

const PAPER_CHICAGO_DRAFT: &str = "# <Title in Title Case>\n\n\
> Curator template: **Academic Paper — Chicago 17th/18th Edition**  \n\
> Formatting: 12pt serif font, double-spaced, 1 inch margins. Choose **Notes-Bibliography** (humanities) or **Author-Date** (sciences) and stay consistent.\n\n\
**Author**: <First Last>  \n\
**Institution**: <…>  \n\
**Date**: <Month YYYY>\n\n\
## Abstract (optional)\n\n\
<Concise summary.>\n\n\
## Introduction\n\n\
<Establish context. Notes-Bibliography uses superscript footnote markers ¹; Author-Date uses parenthetical (Smith 2023, 45).>\n\n\
## Main Sections (use descriptive headings)\n\n\
<Chicago accepts five heading levels; keep them descriptive and parallel.>\n\n\
### Sub-section\n\n\
<…>\n\n\
## Conclusion\n\n\
<…>\n\n\
## Notes (Notes-Bibliography style only)\n\n\
<First reference full citation; subsequent shortened. Example:\n\
1. John Smith, *Title of Book* (City: Publisher, 2023), 45.\n\
2. Smith, *Title*, 46.>\n\n\
## Bibliography\n\n\
<Alphabetical, hanging indent. Examples:\n\
- Smith, John. *Title of Book*. City: Publisher, 2023.\n\
- Lee, Karen. \"Article Title.\" *Journal Name* 12, no. 3 (2024): 45–67. https://doi.org/10.xxxx/yyyy.\n\
Every footnote must trace back to a `[Sn]` entry in `sources.md`.>\n";

const PAPER_GB7714_DRAFT: &str = "# <论文题目（中文）>\n\n\
> Curator 模板：**学术论文 — GB/T 7714（2015 / 2026 年 7 月起 2025 版）**  \n\
> 格式：宋体小四正文，1.5 倍行距，A4 纸，页边距上下 2.54cm/左右 3.17cm；中英文摘要并列。\n\n\
**作者**：<姓名 1>，<姓名 2>  \n\
**作者单位**：<机构名称, 城市 邮编>  \n\
**通讯作者**：<姓名>，<email>\n\n\
## 摘要\n\n\
<中文摘要 150–500 字，第三人称，包含研究目的、方法、结果、结论。>\n\n\
**关键词**：<词 1>；<词 2>；<词 3>；<词 4>\n\n\
**中图分类号**：<TPxxx>  \n\
**文献标识码**：A\n\n\
## Abstract\n\n\
<English abstract — third person; objective / method / result / conclusion.>\n\n\
**Key words**: <kw1>; <kw2>; <kw3>; <kw4>\n\n\
## 1 引言\n\n\
<研究背景、问题陈述、研究意义、文献综述、本文结构。文内引用使用顺序编码制 `[1]`，与「参考文献」一一对应。>\n\n\
## 2 相关工作\n\n\
<分组综述前人工作，明确指出本文待解决的研究空白。>\n\n\
## 3 方法\n\n\
<理论建模 / 算法 / 系统设计；含算式、伪代码、模块图。引用工作区代码使用 `path:lineStart-lineEnd`。>\n\n\
## 4 实验与分析\n\n\
<数据集、基线、评价指标、硬件配置、消融实验。表格请使用 Markdown 表格语法，DOCX 渲染时会转为标准三线表。>\n\n\
| 实验设置 | 基线 | 本文方法 | 提升 |\n\
|---------|------|---------|------|\n\
| <…>     | <…> | <…>     | <…>  |\n\n\
## 5 讨论\n\n\
<解释结果、有效性威胁、失败模式。>\n\n\
## 6 结论\n\n\
<总结贡献、给出未来工作方向。>\n\n\
## 致谢\n\n\
<资助、机构与个人致谢。>\n\n\
## 参考文献\n\n\
<按 GB/T 7714-2015 著录（2025 版新增预印本/数据集著录规则也已兼容）。按文中出现顺序编号。文献类型代码：M 专著 / J 期刊 / C 论文集 / D 学位论文 / R 报告 / S 标准 / P 专利 / EB/OL 电子文献。\n\n\
示例：\n\
[1] 张三, 李四. 论文题目[J]. 期刊名, 2024, 12(3): 45-67. DOI: 10.xxxx/yyyy.\n\
[2] Smith J A, Lee K. Title of paper[C]//Proc. of XXX Conf. City: Publisher, 2023: 100-115.\n\
[3] OpenAI. GPT-X Technical Report[EB/OL]. (2024-05-10)[2026-05-22]. https://arxiv.org/abs/2405.xxxxx.\n\n\
请把 `sources.md` 中的每条 `[Sn]` 都映射为这里的 `[n]` 编号。>\n";

const PAPER_BLUEPRINT: &str = "# Implementation Blueprint — <Paper Slug>\n\n\
> Curator template: **Academic Paper ⇒ Reference Implementation**\n\n\
The Agent mode build that follows this blueprint MUST reproduce every quantitative claim in the paper. Treat this document as the contract.\n\n\
## 1. Scope\n\n\
- In-scope deliverables: <…>\n\
- Out-of-scope: <…>\n\
- Reproducibility success criteria (each numeric claim, with tolerance): <…>\n\n\
## 2. System Decomposition\n\n\
- Module A — <path/name> — responsibility: <…>\n\
- Module B — <…>\n\
- Module C — <…>\n\n\
## 3. Datasets & External Resources\n\n\
- Dataset URLs and licences\n\
- Pretrained model checkpoints (URL + SHA256)\n\
- Required environment variables / API keys\n\n\
## 4. Build & Run\n\n\
- Toolchain: <language, version, package manager>\n\
- Setup commands:\n\n\
```bash\n<setup commands>\n```\n\n\
- Run commands:\n\n\
```bash\n<run commands>\n```\n\n\
## 5. Verification\n\n\
- Evaluation harness paths\n\
- Metrics threshold table (must reproduce paper Table N within ±x%)\n\
- Smoke command:\n\n\
```bash\n<smoke command>\n```\n\n\
## 6. Risks & Mitigations\n\n\
- Risk: <…> → Mitigation: <…>\n";

// ============================================================================
// SOLUTION TEMPLATES
// ============================================================================

const SOLUTION_FUNCTIONAL_DRAFT: &str = "# <解决方案标题>\n\n\
> Curator 模板：**工程解决方案 — 功能设计为核心**  \n\
> 编写要求：重点阐述「核心功能模块」的功能描述、技术原理、关键指标、数据/接口、实现要点；所有表格请使用 Markdown 表格语法（| ... |），DOCX 渲染时会转换为标准三线表。\n\
> **硬约束**：禁止贴具体源码（`go`/`java`/`python`/`rust`/`c`/`cpp` 等语言标签的代码块）；禁止 `path/file.ext:行号` 形式的源码引用；禁止以具名开源项目名直接称呼（用「某 Go 语言的 LLM 网关开源项目」等中性描述替代）。允许的代码块：`bash`/`sh`（部署命令）、`yaml`/`toml`/`json`/`ini`/`nginx`（配置样本）、`mermaid`（图示）、`text`（≤10 行伪代码/Schema）。\n\n\
**项目编号**：<P-YYYY-NNN>  \n\
**版本**：v0.1  \n\
**作者**：<姓名>  \n\
**评审**：<干系人列表>  \n\
**日期**：<YYYY-MM-DD>\n\n\
## 一、系统概述\n\n\
<2-4 段文字：解决的业务问题、整体技术路径、可交付物、与现有系统的关系。引用 `[Sn]` 来自 `sources.md`。>\n\n\
## 二、总体架构\n\n\
### 2.1 架构图\n\n\
<Mermaid / ASCII 架构图；说明分层（接入层 / 业务层 / 数据层 / 基础设施）。>\n\n\
### 2.2 关键组件清单\n\n\
| 序号 | 组件名称 | 角色 | 技术选型 | 输入 | 输出 |\n\
|------|---------|------|---------|------|------|\n\
| 1    | <组件>  | <角色> | <技术>  | <…>  | <…>  |\n\
| 2    | <…>    | <…>   | <…>    | <…>  | <…>  |\n\n\
### 2.3 关键技术选型与理由（架构决策记录 ADR 摘要）\n\n\
| 决策点 | 备选方案 | 选定方案 | 理由 | 引用 |\n\
|--------|---------|---------|------|------|\n\
| <选型 1> | <A/B/C> | <X>    | <…> | `[Sn]` |\n\n\
## 三、核心功能模块设计\n\n\
> 本节是文档的重点。**每个模块必须包含「功能描述 / 技术原理 / 关键指标 / 数据与接口 / 实现要点」五个子段。**\n\n\
### 3.1 <模块 1 名称>\n\n\
**功能描述**\n\n\
<以用户视角说明该模块做什么；包含输入条件、行为边界、输出形态。>\n\n\
**技术原理**\n\n\
<算法 / 协议 / 数据结构 / 关键参数；可用文字、公式、Mermaid 流程图描述。如需逻辑示意，写 ≤10 行的 `text` 伪代码块；禁止贴真实业务源码、禁止 `path/file.ext:行号` 形式的源码引用、禁止点名具体开源项目。>\n\n\
**关键指标**\n\n\
| 指标 | 目标值 | 测试方法 |\n\
|------|-------|---------|\n\
| 吞吐量 | ≥ <值> | <方法> |\n\
| 平均延迟 | ≤ <值> ms | <方法> |\n\
| P99 延迟 | ≤ <值> ms | <方法> |\n\
| 准确率 | ≥ <值>% | <方法> |\n\n\
**数据与接口**\n\n\
- 输入 schema：<JSON/Proto/SQL 定义>\n\
- 输出 schema：<…>\n\
- 对外 API：`POST /api/v1/<module>` — 请求体 <…>，响应体 <…>\n\
- 事件 / 消息：`<topic>` — payload <…>\n\n\
**实现要点**\n\n\
- 依赖库 / 框架：<…>\n\
- 异常 / 重试 / 限流策略：<…>\n\
- 监控埋点（必出指标）：<…>\n\n\
### 3.2 <模块 2 名称>\n\n\
<按照 3.1 同样的五子段填写。>\n\n\
### 3.3 <模块 3 名称>\n\n\
<按照 3.1 同样的五子段填写。>\n\n\
## 四、数据模型\n\n\
### 4.1 实体关系图\n\n\
<ER 图 / 表关系图。>\n\n\
### 4.2 关键表结构\n\n\
| 表名 | 字段 | 类型 | 约束 | 说明 |\n\
|------|------|------|------|------|\n\
| <table> | id | bigint | PK | <…> |\n\
| <table> | <…> | <…> | <…> | <…> |\n\n\
## 五、对外接口（API）\n\n\
| Method | Path | 功能 | 鉴权 | 限流 |\n\
|--------|------|------|------|------|\n\
| POST   | /api/v1/<…> | <功能> | JWT | 100 QPS |\n\n\
## 六、配置与运行环境\n\n\
| 类别 | 推荐配置 | 最低配置 | 说明 |\n\
|------|---------|---------|------|\n\
| 操作系统 | <…>   | <…>    | <…>  |\n\
| 运行时   | <…>   | <…>    | <…>  |\n\
| 数据库   | <…>   | <…>    | <…>  |\n\
| 缓存     | <…>   | <…>    | <…>  |\n\
| 消息队列 | <…>   | <…>    | <…>  |\n\n\
## 七、部署方案\n\n\
- 部署拓扑：<图示或文字>\n\
- 部署步骤：\n\n\
```bash\n<deploy commands>\n```\n\n\
- 回滚策略：<…>\n\
- 灰度 / 蓝绿 / 金丝雀：<…>\n\n\
## 八、性能与可扩展性\n\n\
| 维度 | 当前目标 | 一年内目标 | 扩展方式 |\n\
|------|---------|-----------|---------|\n\
| 并发用户 | ≥ <…> | ≥ <…> | <水平扩展/分片> |\n\
| QPS    | ≥ <…> | ≥ <…> | <…>            |\n\
| 数据规模 | <…>  | <…>  | <…>            |\n\n\
## 九、可观测性与运维\n\n\
- 关键指标（Metrics）：<…>\n\
- 日志（Logs）：<结构化字段列表>\n\
- 链路追踪（Traces）：<采样策略>\n\
- 告警规则：<…>\n\n\
## 十、安全与合规\n\n\
- 身份鉴权与授权\n\
- 数据加密（传输 / 存储）\n\
- 审计日志\n\
- 合规要求（如 GDPR / 等保 / ISO 27001）\n\n\
## 十一、风险与权衡\n\n\
| 风险 | 等级 | 触发条件 | 缓解措施 | 负责人 |\n\
|------|------|---------|---------|--------|\n\
| <…> | 高/中/低 | <…> | <…> | <…> |\n\n\
## 十二、项目计划与里程碑\n\n\
| 阶段 | 起止 | 关键交付物 | 验收标准 |\n\
|------|------|-----------|---------|\n\
| Phase 1 | <…> | <…>       | <…>     |\n\
| Phase 2 | <…> | <…>       | <…>     |\n\n\
## 十三、验收标准\n\n\
- 功能验收：<可验证条目>\n\
- 性能验收：<量化指标>\n\
- 安全验收：<…>\n\
- 文档验收：<…>\n\n\
## 十四、附录\n\n\
- 名词表 / 缩略语\n\
- 参考资料（同步至 `sources.md`）\n\
- 历史版本变更记录\n";

const SOLUTION_FUNCTIONAL_BLUEPRINT: &str = "# Implementation Blueprint — <Solution Slug>\n\n\
> Curator template: **Solution Document ⇒ Engineering Delivery**\n\n\
Agent mode will follow this blueprint verbatim. Make every clause executable.\n\n\
## 1. Delivery Contract\n\n\
- End-state description: <…>\n\
- Verifiable acceptance commands:\n\n\
```bash\n<acceptance commands>\n```\n\n\
- Deployment target: <env / platform>\n\n\
## 2. System Decomposition\n\n\
- Service / module A — <path> — responsibility: <…>\n\
- Service / module B — <…>\n\
- Shared infra: <db, queue, cache, …>\n\n\
## 3. Data Model\n\n\
<Tables, schemas, message contracts.>\n\n\
## 4. APIs & Interfaces\n\n\
| Method | Path | Purpose | Auth |\n\
|--------|------|---------|------|\n\
| POST   | /api/v1/<…> | <…> | <…> |\n\n\
## 5. Phased Execution\n\n\
- Phase 1: <track + acceptance>\n\
- Phase 2: <track + acceptance>\n\
- Phase 3: <track + acceptance>\n\n\
## 6. Observability & Rollback\n\n\
- Metrics / logs / traces required\n\
- Rollback playbook\n\
- Feature-flag boundaries\n\n\
## 7. Verification\n\n\
- Smoke command:\n\n\
```bash\n<smoke>\n```\n\n\
- Functional acceptance checklist: <…>\n\
- Performance acceptance: <quant thresholds>\n\n\
## 8. Risks\n\n\
- Risk: <…> → Mitigation: <…>\n";

const SOLUTION_GB8567_2006_DRAFT: &str = "# <软件项目名称> 解决方案文档\n\n\
> Curator 模板：**GB/T 8567-2006（计算机软件文档编制规范）**  \n\
> 章节结构对齐 GB/T 8567-2006 招投标/验收交付件；表格使用 Markdown 表格语法，DOCX 渲染为三线表。\n\n\
| 文档信息 | 内容 |\n\
|---------|------|\n\
| 项目编号 | <…>  |\n\
| 版本号   | v1.0 |\n\
| 编制人   | <…>  |\n\
| 评审人   | <…>  |\n\
| 编制日期 | <YYYY-MM-DD> |\n\n\
## 1 引言\n\n\
### 1.1 编写目的\n<本文档面向甲方/评审专家说明 …>\n\n\
### 1.2 背景\n<业务现状、立项依据。>\n\n\
### 1.3 术语和缩略语\n| 缩写 | 全称 | 含义 |\n|------|------|------|\n| <…> | <…> | <…> |\n\n\
### 1.4 参考资料\n<标准、上下游文档，与 `sources.md` 中的 `[Sn]` 对齐。>\n\n\
## 2 总体描述\n\n\
### 2.1 产品定位\n<…>\n\n\
### 2.2 用户特征与运行环境\n<…>\n\n\
### 2.3 假设和约束\n<…>\n\n\
## 3 功能需求\n\n\
> 重点章节：以「功能模块 / 子功能 / 输入输出 / 业务规则 / 异常处理」分级描述。\n\n\
### 3.1 <子系统 1>\n#### 3.1.1 功能描述\n<…>\n#### 3.1.2 输入/输出\n<…>\n#### 3.1.3 业务规则\n<…>\n#### 3.1.4 异常处理\n<…>\n\n\
### 3.2 <子系统 2>\n<…>\n\n\
## 4 非功能需求\n\n\
| 类别 | 指标 | 目标值 | 测试方法 |\n\
|------|------|-------|---------|\n\
| 性能 | 并发用户数 | ≥ <…> | <…> |\n\
| 性能 | 平均响应时间 | ≤ <…> ms | <…> |\n\
| 可靠性 | 可用性 | ≥ 99.9% | <…> |\n\
| 安全性 | <…> | <…> | <…> |\n\n\
## 5 系统设计\n\n\
### 5.1 总体架构\n<图示与说明。>\n\n\
### 5.2 子系统/模块设计\n<参考 SolutionFunctional 模板的「核心功能模块设计」五子段法。>\n\n\
### 5.3 数据库设计\n<E-R 图、关键表结构。>\n\n\
### 5.4 接口设计\n<外部接口、内部接口。>\n\n\
## 6 实施方案\n\n\
### 6.1 项目计划与里程碑\n<…>\n\n\
### 6.2 资源与组织\n<人员、设备、第三方依赖。>\n\n\
### 6.3 部署上线\n<…>\n\n\
## 7 测试方案\n\n\
- 测试类型（功能/性能/安全/兼容/回归）\n\
- 测试用例覆盖率目标\n\
- 测试环境与数据\n\n\
## 8 验收标准\n\n\
- 功能验收\n\
- 性能验收\n\
- 文档验收\n\
- 培训与移交\n\n\
## 9 风险与对策\n\n\
| 风险 | 等级 | 缓解措施 |\n|------|------|---------|\n| <…> | 高/中/低 | <…> |\n\n\
## 10 附录\n\n\
- 缩略语\n\
- 参考资料\n\
- 变更记录\n";

const SOLUTION_GB8567_1988_DRAFT: &str = "# <软件项目名称> 解决方案文档\n\n\
> Curator 模板：**GB/T 8567-1988（13 种核心文档历史规范）**  \n\
> 用于沿用旧标准的项目；正文章节按 1988 版关键文档要素组织。\n\n\
## 1 引言\n### 1.1 编写目的\n<…>\n### 1.2 背景\n<…>\n### 1.3 定义\n<…>\n### 1.4 参考资料\n<…>\n\n\
## 2 任务概述\n### 2.1 目标\n<…>\n### 2.2 用户的特点\n<…>\n### 2.3 假定和约束\n<…>\n\n\
## 3 需求规定\n### 3.1 功能需求\n<参考 SolutionFunctional 五子段法。>\n### 3.2 性能需求\n<…>\n### 3.3 输入输出要求\n<…>\n### 3.4 数据管理能力要求\n<…>\n### 3.5 故障处理要求\n<…>\n\n\
## 4 运行环境规定\n### 4.1 硬件设备\n<…>\n### 4.2 支持软件\n<…>\n### 4.3 接口\n<…>\n### 4.4 控制\n<…>\n\n\
## 5 系统设计\n### 5.1 总体设计\n<…>\n### 5.2 模块设计\n<…>\n### 5.3 数据结构设计\n<…>\n\n\
## 6 实施与维护\n### 6.1 进度安排\n<…>\n### 6.2 维护计划\n<…>\n\n\
## 7 附录\n<…>\n";

const SOLUTION_IEEE830_DRAFT: &str = "# Software Requirements Specification (SRS)\n\n\
> Curator template: **IEEE Std 830-1998**  \n\
> Recommended Practice for Software Requirements Specifications.\n\n\
| Field | Value |\n\
|-------|-------|\n\
| Document ID | <…> |\n\
| Version     | 1.0 |\n\
| Author      | <…> |\n\
| Date        | <YYYY-MM-DD> |\n\n\
## 1. Introduction\n\n\
### 1.1 Purpose\n<…>\n\n### 1.2 Scope\n<…>\n\n\
### 1.3 Definitions, Acronyms, and Abbreviations\n<table>\n\n### 1.4 References\n<map to `[Sn]` in `sources.md`>\n\n\
### 1.5 Overview\n<…>\n\n\
## 2. Overall Description\n\n\
### 2.1 Product Perspective\n<context diagram, interfaces with external systems, hardware/software/communications interfaces, memory constraints, operations>\n\n\
### 2.2 Product Functions\n<top-level summary, ordered by priority>\n\n\
### 2.3 User Characteristics\n<…>\n\n\
### 2.4 Constraints\n<regulatory, hardware, technology, safety>\n\n\
### 2.5 Assumptions and Dependencies\n<…>\n\n\
### 2.6 Apportioning of Requirements\n<future-release versus this-release>\n\n\
## 3. Specific Requirements\n\n\
> Each functional requirement uses the form `FR-NNN: <The system shall …>` with a unique ID, source, rationale, and verification method.\n\n\
### 3.1 External Interface Requirements\n#### 3.1.1 User Interfaces\n<…>\n#### 3.1.2 Hardware Interfaces\n<…>\n#### 3.1.3 Software Interfaces\n<…>\n#### 3.1.4 Communications Interfaces\n<…>\n\n\
### 3.2 Functional Requirements\n\n\
| ID | Requirement | Source | Priority | Verification |\n\
|----|------------|--------|----------|--------------|\n\
| FR-001 | The system shall <…> | <…> | High | Test |\n\
| FR-002 | The system shall <…> | <…> | High | Demo |\n\n\
### 3.3 Performance Requirements\n<quantitative thresholds>\n\n\
### 3.4 Design Constraints\n<…>\n\n\
### 3.5 Software System Attributes\n<reliability, availability, security, maintainability, portability>\n\n\
### 3.6 Other Requirements\n<…>\n\n\
## 4. Appendices\n\n\
- Glossary\n- Analysis Models\n- ToBe Decided list\n";

const SOLUTION_ISO29148_DRAFT: &str = "# System / Software Requirements Specification (SyRS/SRS)\n\n\
> Curator template: **ISO/IEC/IEEE 29148:2011 — Systems and software engineering — Life cycle processes — Requirements engineering**  \n\
> Supersedes IEEE 830-1998. Use when modern, life-cycle-aligned requirements engineering is required.\n\n\
| Document | Value |\n\
|----------|-------|\n\
| Title    | <…>   |\n\
| Author   | <…>   |\n\
| Version  | 1.0   |\n\
| Date     | <YYYY-MM-DD> |\n\n\
## 1. Introduction\n\n\
### 1.1 Document Purpose\n<…>\n\n### 1.2 Intended Audience\n<…>\n\n### 1.3 Definitions\n<…>\n\n### 1.4 Document Conventions\n<…>\n\n### 1.5 References\n<…>\n\n\
## 2. Stakeholder Needs and Requirements\n\n\
### 2.1 Business Mission, Objectives and Goals\n<…>\n\n### 2.2 Business Environment\n<…>\n\n### 2.3 Stakeholders\n| ID | Stakeholder | Role | Concerns | Authority |\n|----|------------|------|---------|-----------|\n| <S1> | <…> | <…> | <…> | <…> |\n\n\
### 2.4 Operational Concept (OpsCon)\n<scenarios, use-cases>\n\n\
### 2.5 Stakeholder Requirements\n| ID | Requirement | Stakeholder | Priority |\n|----|------------|-------------|----------|\n| SR-001 | The user shall be able to <…> | <…> | High |\n\n\
## 3. System Requirements\n\n\
### 3.1 System Context\n<system boundary diagram, interfaces, external entities>\n\n\
### 3.2 Functional Requirements\n| ID | Requirement | Source SR | Verification |\n|----|------------|-----------|--------------|\n| FR-001 | The system shall <…> | SR-001 | Test |\n\n\
### 3.3 Usability Requirements\n<…>\n\n### 3.4 Performance Requirements\n<…>\n\n### 3.5 System Interface Requirements\n<…>\n\n### 3.6 System Operations\n<…>\n\n### 3.7 System Modes and States\n<…>\n\n### 3.8 Physical Characteristics\n<…>\n\n### 3.9 Environmental Conditions\n<…>\n\n### 3.10 System Security\n<…>\n\n### 3.11 Information Management\n<…>\n\n### 3.12 Policies and Regulations\n<…>\n\n### 3.13 System Life-cycle Sustainment\n<…>\n\n### 3.14 Packaging, Handling, Shipping, Transportation\n<…>\n\n\
## 4. Verification\n\n\
| FR/SR | Verification Method | Acceptance Criterion |\n|-------|--------------------|--------------------|\n| FR-001 | Test | <…> |\n\n\
## 5. Supporting Information\n\n\
- Assumptions, dependencies, traceability matrix.\n";

const SOLUTION_ISO42010_DRAFT: &str = "# Software Architecture Description (AD)\n\n\
> Curator template: **ISO/IEC/IEEE 42010 — Systems and software engineering — Architecture description**  \n\
> Uses multiple **viewpoints** to describe an architecture from different stakeholder perspectives.\n\n\
| Field | Value |\n\
|-------|-------|\n\
| System name | <…> |\n\
| Architect   | <…> |\n\
| Date        | <YYYY-MM-DD> |\n\
| Version     | 1.0 |\n\n\
## 1. Introduction\n\n\
### 1.1 Purpose\n<…>\n\n### 1.2 Scope\n<…>\n\n### 1.3 Glossary\n<…>\n\n### 1.4 References\n<…>\n\n\
## 2. Stakeholders and Concerns\n\n\
| Stakeholder | Concerns |\n|-------------|----------|\n| Architect | Modifiability, deployability, performance |\n| PM | Cost, schedule, risk |\n| Ops | Observability, recoverability, security |\n\n\
## 3. Viewpoints\n\n\
| ID | Viewpoint | Stakeholders Addressed | Concerns | Modeling Conventions |\n|----|-----------|-----------------------|---------|---------------------|\n| VP-Logical | Logical | Architect, Dev | functional decomposition | UML class/component |\n| VP-Process | Process | Ops | concurrency, performance | sequence/activity |\n| VP-Deployment | Deployment | Ops | physical topology | deployment diagram |\n| VP-Data | Data | DBA, BI | data flow, persistence | ER, lineage |\n\n\
## 4. Views\n\n\
### 4.1 Logical View\n<class / component / module decomposition>\n\n\
### 4.2 Process View\n<runtime structures, threads, processes, concurrency, queues>\n\n\
### 4.3 Deployment View\n<nodes, network, environment, hosting>\n\n\
### 4.4 Data View\n<conceptual data model, persistence, migration>\n\n\
### 4.5 Use-case / Scenarios\n<key end-to-end scenarios stitched across views>\n\n\
## 5. Correspondence Rules\n\n\
<rules linking elements across views, e.g. every component in Logical View must map to ≥1 process in Process View and ≥1 node in Deployment View>\n\n\
## 6. Architecture Rationale (Decision Log)\n\n\
| ID | Decision | Alternatives | Rationale | Status |\n|----|----------|-------------|-----------|--------|\n| ADR-001 | <…> | A/B/C | <…> | Accepted |\n\n\
## 7. Architecture Risks & Quality Attribute Scenarios\n\n\
| QA | Scenario | Response | Measurement |\n|----|----------|----------|-------------|\n| Performance | 10k concurrent users issue search | system returns within 200ms | P99 latency ≤ 200ms |\n| Availability | Single AZ outage | failover to secondary AZ | RTO < 5 min |\n";

const SOLUTION_IEEE1016_DRAFT: &str = "# Software Design Description (SDD)\n\n\
> Curator template: **IEEE Std 1016-2009 — Software design descriptions**  \n\
> Captures detailed software design as multiple views aligned to stakeholder concerns.\n\n\
| Field | Value |\n|-------|-------|\n| System | <…> |\n| Author | <…> |\n| Version | 1.0 |\n| Date | <YYYY-MM-DD> |\n\n\
## 1. Design Identification\n\n\
- Title / version / authors / approvers.\n\
- Issuing organisation, date, reference to requirements baseline.\n\n\
## 2. Design Stakeholders and Concerns\n\n\
| Stakeholder | Concerns |\n|-------------|----------|\n| <…> | <…> |\n\n\
## 3. Design Views\n\n\
### 3.1 Context View\n<system context, external interactions>\n\n\
### 3.2 Composition View\n<modules, components, packages>\n\n\
### 3.3 Logical View\n<class / state-machine / data abstractions>\n\n\
### 3.4 Information View\n<persistent data, schemas, integrity, retention>\n\n\
### 3.5 Patterns Use View\n<applied design patterns and where>\n\n\
### 3.6 Interface View\n| Interface | Provider | Consumer | Contract |\n|-----------|----------|---------|----------|\n| <…>      | <…>      | <…>     | <…>     |\n\n\
### 3.7 Interaction View\n<sequence diagrams for key operations>\n\n\
### 3.8 State Dynamics View\n<system / component state machines>\n\n\
### 3.9 Algorithm View\n<critical algorithms, pseudo-code, complexity>\n\n\
### 3.10 Resource View\n<memory, CPU, network budgets per component>\n\n\
## 4. Design Rationale\n\n\
| ID | Decision | Alternatives | Rationale |\n|----|----------|-------------|-----------|\n| DD-001 | <…> | A/B | <…> |\n\n\
## 5. Traceability\n\n\
- Map each design element (DD-NNN) back to a requirement (FR/SR-NNN) and forward to a test case (TC-NNN).\n";

const SOLUTION_ISO12207_DRAFT: &str = "# Software Lifecycle Process Plan\n\n\
> Curator template: **ISO/IEC/IEEE 12207 — Systems and software engineering — Software lifecycle processes**  \n\
> Organises the project documentation along the standard's four process groups.\n\n\
| Field | Value |\n|-------|-------|\n| Project | <…> |\n| Plan owner | <…> |\n| Version | 1.0 |\n| Date | <YYYY-MM-DD> |\n\n\
## 1. Agreement Processes\n\n\
### 1.1 Acquisition\n<scope of supply, RFP/contract terms>\n\n\
### 1.2 Supply\n<supplier obligations, deliverables, acceptance>\n\n\
## 2. Organizational Project-Enabling Processes\n\n\
### 2.1 Life Cycle Model Management\n<life-cycle model selected: Agile / V-Model / Hybrid>\n\n\
### 2.2 Infrastructure Management\n<dev/test/prod environments>\n\n\
### 2.3 Portfolio Management\n<…>\n\n\
### 2.4 Human Resource Management\n<roles, training>\n\n\
### 2.5 Quality Management\n<QA strategy, metrics>\n\n\
### 2.6 Knowledge Management\n<…>\n\n\
## 3. Project Processes\n\n\
### 3.1 Project Planning\n<WBS, milestones, dependencies>\n\n\
### 3.2 Project Assessment and Control\n<status reporting cadence, change control>\n\n\
### 3.3 Decision Management\n<decision log>\n\n\
### 3.4 Risk Management\n<register>\n\n\
### 3.5 Configuration Management\n<branching, release versioning>\n\n\
### 3.6 Information Management\n<repository, retention>\n\n\
### 3.7 Measurement\n<KPIs>\n\n\
### 3.8 Quality Assurance\n<…>\n\n\
## 4. Technical Processes\n\n\
### 4.1 Business or Mission Analysis\n<…>\n\n\
### 4.2 Stakeholder Needs and Requirements Definition\n<…>\n\n\
### 4.3 System/Software Requirements Definition\n<reference to SRS doc>\n\n\
### 4.4 Architecture Definition\n<reference to AD doc>\n\n\
### 4.5 Design Definition\n<reference to SDD doc>\n\n\
### 4.6 System Analysis\n<…>\n\n\
### 4.7 Implementation\n<…>\n\n\
### 4.8 Integration\n<…>\n\n\
### 4.9 Verification\n<…>\n\n\
### 4.10 Transition\n<…>\n\n\
### 4.11 Validation\n<…>\n\n\
### 4.12 Operation\n<…>\n\n\
### 4.13 Maintenance\n<…>\n\n\
### 4.14 Disposal\n<…>\n";

// ============================================================================
// TECH REPORT TEMPLATES
// ============================================================================

const TECH_REPORT_DRAFT: &str = "# <Technical Report Title>\n\n\
> Curator template: **Technical Report**\n\n\
**Authors**: <name(s)>  \n\
**Report ID**: <internal id>  \n\
**Date**: <YYYY-MM-DD>\n\n\
## Abstract\n\n\
<Concise summary of objective, methodology, key results, and recommendation.>\n\n\
## 1. Background\n\n\
<Domain context, related prior efforts. Cite sources via `[Sn]`.>\n\n\
## 2. Objectives\n\n\
- Objective 1: <…>\n\
- Objective 2: <…>\n\
- Objective 3: <…>\n\n\
## 3. Methodology\n\n\
<Approach taken, tools/frameworks used, experiment design, measurement protocol. Reference workspace artifacts via `path:lineStart-lineEnd`.>\n\n\
## 4. Results\n\n\
| Experiment | Configuration | Outcome | Notes |\n\
|------------|---------------|---------|-------|\n\
| <…>        | <…>           | <…>     | <…>   |\n\n\
## 5. Analysis\n\n\
<Interpretation, surprising findings, limitations.>\n\n\
## 6. Recommendations\n\n\
- <Concrete recommendation 1>\n\
- <Concrete recommendation 2>\n\
- <Concrete recommendation 3>\n\n\
## 7. References\n\n\
<Imported from `sources.md`.>\n\n\
## 8. Appendix\n\n\
<Raw data, configs, supplementary plots, glossary.>\n";

const TECH_REPORT_BLUEPRINT: &str = "# Implementation Blueprint — <Report Slug>\n\n\
> Curator template: **Technical Report ⇒ Reproducible Engineering Asset**\n\n\
## 1. Reproducibility Contract\n\n\
- Environment definition (Dockerfile / nix flake / requirements.txt path)\n\
- Seed / config snapshot location\n\
- One-shot reproduction command:\n\n\
```bash\n<one-shot command>\n```\n\n\
## 2. Module Layout\n\n\
- <crate / package>: responsibility, public surface\n\
- <…>\n\n\
## 3. Data & Telemetry\n\n\
- Required datasets and where to mount them\n\
- Telemetry channels emitted by the implementation\n\n\
## 4. Verification Matrix\n\n\
- Each numeric result in §4 of the report MUST be reproducible by:\n  - command:\n\n\
```bash\n<cmd>\n```\n\n\
  - tolerance: <±x%>\n\n\
## 5. Risk Register\n\n\
- Risk: <…> → Mitigation: <…>\n";
