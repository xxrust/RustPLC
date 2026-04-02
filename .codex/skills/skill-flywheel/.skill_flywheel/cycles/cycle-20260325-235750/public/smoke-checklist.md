# Smoke Checklist

初始化完成后，至少检查这些文件是否存在：

- `context/program.md`
- `context/task.md`
- `logs/pain-points.md`
- `logs/pain-points.json`
- `logs/root-cause.md`
- `logs/root-cause.json`
- `logs/decision.md`
- `logs/decision.json`
- `logs/run-index.json`
- `logs/synthesis.json`
- `prompts/agent1.md`
- `prompts/agent2.md`
- `prompts/agent3.md`

如果以上文件齐全，再检查：

- `public/README_BOUNDARY.md` 是否明确了读取边界
- `prompts/agent2.md` 是否引用了研究程序路径
- `prompts/agent3.md` 是否要求写入决策记录
- `manifest.json` 是否记录了结构化日志路径
