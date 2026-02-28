"""
Resources: docs/*.md
"""

from mcp.server.fastmcp import FastMCP
from pathlib import Path

DOCS_DIR = Path(__file__).parent.parent.parent / "docs" / "已实现"


def register_doc_resources(mcp: FastMCP):

    @mcp.resource("rustplc://docs/list")
    def list_docs() -> str:
        """列出所有可用的技术文档"""
        if not DOCS_DIR.exists():
            return "Docs directory not found."

        md_files = sorted(DOCS_DIR.glob("*.md"))
        if not md_files:
            return "No documentation files found."

        result = "# RustPLC 技术文档\n\n"

        # 分类展示
        categories = {
            "DSL 语法与验证": [
                "dsl_verification_boundary.md",
                "dsl_compute_rust_plan.md",
                "extern_function_mvp_spec.md",
                "extern_function_development_guide.md",
            ],
            "设备与拓扑": [
                "device-library-design.md",
                "topology_semantic_spec_v1.md",
                "composite_device_port_semantics.md",
            ],
            "仿真与测试": [
                "scenario_playbook.md",
                "dsl-sil-verification.md",
                "hil_regression.md",
            ],
            "部署与运维": [
                "board_rp2040.md",
                "commissioning_playbook.md",
                "diagnostics_backend_methodology.md",
            ],
        }

        for category, files in categories.items():
            result += f"\n## {category}\n"
            for filename in files:
                if (DOCS_DIR / filename).exists():
                    result += f"- `{filename}` - 访问: `@rustplc://docs/{filename}`\n"

        result += "\n## 其他文档\n"
        for f in md_files:
            if not any(f.name in cat_files for cat_files in categories.values()):
                result += f"- `{f.name}` - 访问: `@rustplc://docs/{f.name}`\n"

        return result

    @mcp.resource("rustplc://docs/{filename}")
    def get_doc(filename: str) -> str:
        """
        获取指定的技术文档内容。

        常用文档：
        - extern_function_mvp_spec.md - Extern 函数语法规范（冻结版）
        - extern_function_development_guide.md - Extern 函数开发指南
        - dsl_verification_boundary.md - DSL 形式化验证边界论证
        - device-library-design.md - 设备库设计
        - scenario_playbook.md - 场景系统 playbook
        """
        file_path = DOCS_DIR / filename
        if not file_path.exists():
            return f"Documentation file '{filename}' not found. Use @rustplc://docs/list to see available files."

        try:
            content = file_path.read_text(encoding="utf-8")
            return f"# {filename}\n\n{content}"
        except Exception as e:
            return f"Error reading {filename}: {str(e)}"
