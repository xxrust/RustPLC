# plc-system Workflow

Use this file when the caller needs a stable `.system.md` before PLC generation.

## Goal

Produce a confirmed system contract that downstream PLC generation can trust.

## Flow

1. read the requirement and propose a concrete interpretation first
2. ask only 1 to 3 blocking questions if safety, task partition, or fault handling remain ambiguous
3. produce `main.system.md` with stable sections
4. record explicit assumptions when the user has not yet confirmed details
5. end with a clean handoff to `plc-gen`

## Response Discipline

Do not send back a shopping list of everything the user forgot.
Default to a concrete recommendation first, then ask at most 3 pointed confirmations.

Use this shape when information is incomplete but still actionable:

```text
Current recommendation: ...
Reason: ...
Please confirm:
1. ...
2. ...
3. ...
```

Only refuse to draft when a responsible recommendation is impossible even with conservative defaults.

## Blocking Topics

Treat these as high-impact:

- safety class and failure consequence
- start mode and cycle mode
- startup, reset, and e-stop policy
- manual intervention points
- task partition and blocking isolation
- shared-resource conflicts
- timeout and fault routing expectations

Do not spend the first turn on exact I/O numbering.
