import pathlib
root = pathlib.Path('out/skill_flywheel/plc_gen_wafer_loader/plc')
files = list(root.glob('target_semantics_fragments/auto/*.plcfrag'))
issues = []
for path in files:
    task = None
    steps = {}
    targets = []
    for lineno, line in enumerate(path.read_text().splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith('task '):
            task = stripped.split()[1].rstrip(':')
            steps.setdefault(task, set())
        elif stripped.startswith('step '):
            step_name = stripped.split()[1].rstrip(':')
            if task:
                steps[task].add(step_name)
        if 'goto ' in stripped:
            parts = stripped.split('goto ')[1:]
            for part in parts:
                target = part.split()[0]
                if target.endswith(':'):
                    target = target[:-1]
                if '.' in target:
                    targets.append((target, path, lineno))
    for target, path, lineno in targets:
        task_name, step_name = target.split('.', 1)
        if step_name not in steps.get(task_name, set()):
            issues.append((path, lineno, target))
print('Found', len(issues), 'missing goto targets')
for path, lineno, target in issues:
    print(path, lineno, target)
