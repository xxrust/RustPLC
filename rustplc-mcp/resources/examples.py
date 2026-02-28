"""
Resources: examples/*.plc
"""

from mcp.server.fastmcp import FastMCP
from pathlib import Path

EXAMPLES_DIR = Path(__file__).parent.parent.parent / "examples"


def register_example_resources(mcp: FastMCP):

    @mcp.resource("rustplc://examples/list")
    def list_examples() -> str:
        """列出所有可用的 .plc 示例文件"""
        if not EXAMPLES_DIR.exists():
            return "Examples directory not found."

        plc_files = sorted(EXAMPLES_DIR.glob("*.plc"))
        if not plc_files:
            return "No .plc examples found."

        result = "# RustPLC 示例文件\n\n"
        for f in plc_files:
            result += f"- `{f.name}` - 访问: `@rustplc://examples/{f.name}`\n"

        return result

    @mcp.resource("rustplc://examples/{filename}")
    def get_example(filename: str) -> str:
        """
        获取指定的 .plc 示例文件内容。

        常用示例：
        - two_cylinder.plc - 双气缸顺序动作（基础）
        - assembly_station.plc - 装配站（多设备协同）
        - pid_loop.plc - PID 闭环控制
        - nuclear_coolant_isolation.plc - 核电站隔离阀（SIL3 高安全）
        - quadratic_fit.plc - 二次函数拟合（复杂计算）
        """
        file_path = EXAMPLES_DIR / filename
        if not file_path.exists():
            return f"Example file '{filename}' not found. Use @rustplc://examples/list to see available files."

        try:
            content = file_path.read_text(encoding="utf-8")
            return f"# {filename}\n\n```plc\n{content}\n```"
        except Exception as e:
            return f"Error reading {filename}: {str(e)}"
