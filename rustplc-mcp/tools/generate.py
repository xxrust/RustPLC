"""
Tools: generate_plc, validate_plc, compile_plc
"""

from mcp.server.fastmcp import FastMCP
from rust_bridge import validate_plc_content, get_ir_json
from pathlib import Path

SKILL_PATH = Path(__file__).parent.parent / ".claude" / "skills" / "plc-gen" / "SKILL.md"


def register_generate_tools(mcp: FastMCP):

    @mcp.tool()
    def get_plc_generation_guide() -> str:
        """
        返回完整的 RustPLC DSL 生成指引（SKILL.md）。
        在开始生成 .plc 文件之前，必须先调用此工具获取生成规则和流程。
        """
        if SKILL_PATH.exists():
            return SKILL_PATH.read_text(encoding="utf-8")
        return "SKILL.md not found. Please ensure the rustplc repository is properly set up."

    @mcp.tool()
    def validate_plc(plc_content: str) -> str:
        """
        验证 .plc 文件内容是否通过 RustPLC 四大验证引擎（Safety/Liveness/Timing/Causality）。

        Args:
            plc_content: 完整的 .plc 文件内容字符串

        Returns:
            验证报告，包含通过/失败状态和详细诊断信息
        """
        result = validate_plc_content(plc_content)
        if result["success"]:
            return f"✅ 验证通过\n\n{result['report']}"
        else:
            return f"❌ 验证失败\n\n{result['report']}"

    @mcp.tool()
    def compile_plc(plc_content: str) -> str:
        """
        编译 .plc 文件并返回 IR JSON 和验证报告。
        适合需要查看内部 IR 结构的高级用户。

        Args:
            plc_content: 完整的 .plc 文件内容字符串

        Returns:
            IR JSON（拓扑图、状态机、约束集、时序模型）+ 验证摘要
        """
        result = get_ir_json(plc_content)
        if result["success"]:
            return f"✅ 编译成功\n\n## 验证报告\n{result['report']}\n\n## IR JSON\n```json\n{result['ir_json']}\n```"
        else:
            return f"❌ 编译失败\n\n{result['report']}"
