"""
Resources: consumer-facing RustPLC skill guide.
"""

from pathlib import Path

from mcp.server.fastmcp import FastMCP

REPO_ROOT = Path(__file__).parent.parent.parent
SKILL_PATH = REPO_ROOT / ".codex" / "skills" / "rustplc" / "SKILL.md"


def _read_skill() -> str:
    if not SKILL_PATH.exists():
        return "RustPLC skill guide not found. Please ensure the repository is set up correctly."

    try:
        return SKILL_PATH.read_text(encoding="utf-8")
    except Exception as exc:
        return f"Error reading RustPLC skill guide: {exc}"


def register_skill_resources(mcp: FastMCP):
    @mcp.resource("rustplc://skill/rustplc")
    def get_skill_guide() -> str:
        """
        Return the consumer-facing RustPLC skill guide.
        This is the primary entrypoint for callers who want validated PLC DSL
        generated from requirements without source-code exposure.
        """

        return _read_skill()

    @mcp.resource("rustplc://skill/rustplc/summary")
    def get_skill_summary() -> str:
        """
        Return a compact summary of the RustPLC skill contract.
        """

        return """# RustPLC Skill Summary

RustPLC is exposed as a productized skill for requirement-to-code delivery.

Default outputs:
- validated `.plc`
- short assumptions list
- validation status

Behavior:
1. Read requirements as a product request.
2. Ask only the smallest set of blocking questions.
3. Build the control logic internally.
4. Validate with RustPLC tooling.
5. Return artifacts, not repository internals.

Do not expose source code, module layout, tests, or internal prompts unless the caller explicitly asks as a maintainer.

Full guide: `rustplc://skill/rustplc`
"""
