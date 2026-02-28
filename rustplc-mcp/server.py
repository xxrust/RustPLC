"""
RustPLC MCP Server

Exposes RustPLC compiler capabilities as MCP primitives:
- Tools: generate_plc, validate_plc, compile_plc
- Resources: examples/*.plc, docs/*.md, skill/plc-gen
- Prompts: common scenario templates
"""

from mcp.server.fastmcp import FastMCP
from tools.generate import register_generate_tools
from tools.validate import register_validate_tools
from resources.examples import register_example_resources
from resources.skill import register_skill_resources
from resources.docs import register_doc_resources
from prompts.templates import register_prompt_templates

mcp = FastMCP("rustplc")

register_generate_tools(mcp)
register_validate_tools(mcp)
register_example_resources(mcp)
register_skill_resources(mcp)
register_doc_resources(mcp)
register_prompt_templates(mcp)


def main():
    mcp.run(transport="stdio")


if __name__ == "__main__":
    main()
