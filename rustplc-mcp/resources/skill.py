"""
Resources: skill/plc-gen (SKILL.md)
"""

from mcp.server.fastmcp import FastMCP
from pathlib import Path

SKILL_PATH = Path(__file__).parent.parent.parent / ".claude" / "skills" / "plc-gen" / "SKILL.md"


def register_skill_resources(mcp: FastMCP):

    @mcp.resource("rustplc://skill/plc-gen")
    def get_skill_guide() -> str:
        """
        获取完整的 RustPLC DSL 生成指引（SKILL.md）。
        包含：
        - 多阶段生成流程（.system.md → 理解工艺 → 推理拓扑 → 推导约束 → 生成 DSL）
        - 完整 DSL 语法参考
        - 验证规则速查
        - 命名约定和默认参数
        - 完整代码示例
        """
        if not SKILL_PATH.exists():
            return "SKILL.md not found. Please ensure the rustplc repository is properly set up."

        try:
            content = SKILL_PATH.read_text(encoding="utf-8")
            return content
        except Exception as e:
            return f"Error reading SKILL.md: {str(e)}"

    @mcp.resource("rustplc://skill/plc-gen/summary")
    def get_skill_summary() -> str:
        """
        获取 SKILL.md 的简要摘要，快速了解生成流程。
        """
        return """# RustPLC DSL 生成流程摘要

## 核心理念
从工程师的自然语言工艺描述，经过多轮对话确认，生成可通过四大验证引擎的 .plc 文件。

## 四阶段流程

### 阶段零：生成系统描述（.system.md）
- 定义项目身份、系统使命、安全等级、运行环境
- 作为所有后续决策的语义锚点
- **必须先完成并经工程师确认**

### 阶段一：理解工艺
- 复述动作时序表
- 确认启动方式、循环模式、初始状态、人工介入点、同步关系
- **等待工程师确认后再进入下一阶段**

### 阶段二：推理设备拓扑
- 推理完整设备清单（PLC、执行机构、传感器、人机交互）
- 确认命名、时序参数、I/O 分配
- **等待工程师确认后再进入下一阶段**

### 阶段三：推导约束
- 推导安全约束（conflicts_with / requires）
- 推导因果链（relation + detects）
- 推导时序约束（must_complete_within）
- **等待工程师确认后再进入下一阶段**

### 阶段四：生成 DSL 并验证
- 组装成 .plc 文件
- 运行编译器验证（Safety/Liveness/Timing/Causality）
- 修复错误直到全部通过
- 展示给工程师做最后确认

## 关键原则
- **不要一次性生成最终结果** - 必须多轮确认
- **不要凭空假设** - 遇到不确定的地方必须提问
- **不要遗漏约束** - 工程师确认的每一条安全关系都要转化为 DSL 约束
- **不要跳过验证** - 生成后必须运行编译器验证

完整指引请访问: @rustplc://skill/plc-gen
"""
