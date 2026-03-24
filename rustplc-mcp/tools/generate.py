"""
Tools: skill guide access, validation, compilation.
"""

from pathlib import Path

from mcp.server.fastmcp import FastMCP

from rust_bridge import get_ir_json, validate_plc_content

REPO_ROOT = Path(__file__).parent.parent.parent
SKILL_PATH = REPO_ROOT / ".codex" / "skills" / "rustplc" / "SKILL.md"


def _read_skill() -> str:
    if SKILL_PATH.exists():
        return SKILL_PATH.read_text(encoding="utf-8")
    return "RustPLC skill guide not found. Please ensure the repository is set up correctly."


def register_generate_tools(mcp: FastMCP):
    @mcp.tool()
    def get_rustplc_skill_guide() -> str:
        """
        Return the consumer-facing RustPLC skill guide.
        Use this before generating PLC DSL from requirements.
        """

        return _read_skill()

    @mcp.tool()
    def validate_plc(plc_content: str) -> str:
        """
        Validate PLC DSL content with RustPLC verification.
        Returns a compact passed/failed result with diagnostics.
        """

        result = validate_plc_content(plc_content)
        if result["success"]:
            return f"[PASS] Validation passed\n\n{result['report']}"
        return f"[FAIL] Validation failed\n\n{result['report']}"

    @mcp.tool()
    def compile_plc(plc_content: str) -> str:
        """
        Compile PLC DSL content and return IR JSON with a validation summary.
        This is intended for advanced callers who explicitly want compiled internals.
        """

        result = get_ir_json(plc_content)
        if result["success"]:
            return (
                f"[PASS] Compile succeeded\n\n## Validation\n{result['report']}\n\n"
                f"## IR JSON\n```json\n{result['ir_json']}\n```"
            )
        return f"[FAIL] Compile failed\n\n{result['report']}"
