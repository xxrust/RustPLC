# Prompt Templates

Use these as shapes, not rigid scripts. Fill in the generated paths from the cycle directory.

## Agent 1: Source-Aware Skill Editor

```text
Use $skill-creator to improve the target skill at <TARGET_SKILL_PATH> for project <PROJECT_NAME>.

You may read the repo source and the target skill.
Real task: <TASK>
Public bundle used by the no-source operator: <PUBLIC_DIR>
Pain points will be recorded at: <PAIN_POINTS_PATH>
Root-cause findings will be recorded at: <ROOT_CAUSE_PATH>

Keep the skill lean. If a blocker is better solved by a public artifact or code change, say so instead of stuffing it into the skill.
```

## Agent 2: No-Source Operator

```text
Use the target skill at <TARGET_SKILL_PATH> to complete this real task:
<TASK>

You must stay inside this public workspace:
<PUBLIC_DIR>

Do not read project source or other protected repo paths. This boundary is procedural; honor it strictly.

Write:
1. your result
2. each blocker or inefficiency
3. the exact missing artifact, command, example, or instruction you wanted

Save the blocker list to <PAIN_POINTS_PATH>.
```

## Agent 3: Source-Aware Root-Cause Analyst

```text
Analyze the task, the blind operator's output, and the repo source as needed.

Task: <TASK>
Pain points: <PAIN_POINTS_PATH>
Target skill: <TARGET_SKILL_PATH>
Repo root: <REPO_ROOT>

Classify each pain point as one of:
- skill-gap
- public-surface-gap
- code-gap
- task-ambiguity

Prefer stable exported artifacts over source-heavy skill additions.

Write findings to <ROOT_CAUSE_PATH>. For every skill-gap, specify the minimal delta Agent 1 should add.
```
