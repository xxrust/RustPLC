"""
Simple test script to verify MCP server functionality
"""

import sys
from pathlib import Path

# Add parent directory to path
sys.path.insert(0, str(Path(__file__).parent))

from rust_bridge import validate_plc_content, RUSTPLC_BIN


def test_rust_bridge():
    """Test Rust compiler bridge"""
    print("=== Testing Rust Bridge ===")
    print(f"RustPLC binary: {RUSTPLC_BIN}")

    # Simple valid PLC program
    valid_plc = """
[topology]
device plc_main: plc {
    purpose: "test controller",
    ports: [X0:digital:consumer]
}

[constraints]

[tasks]
task main:
    step wait:
        allow_indefinite_wait: true
"""

    print("\nValidating simple PLC program...")
    result = validate_plc_content(valid_plc)
    print(f"Success: {result['success']}")
    if result['success']:
        print("[PASS] Validation passed")
    else:
        print("[FAIL] Validation failed")
        print(result['report'])

    return result['success']


def test_resources():
    """Test resource loading"""
    print("\n=== Testing Resources ===")

    from resources.examples import EXAMPLES_DIR
    from resources.docs import DOCS_DIR
    from resources.skill import SKILL_PATH

    print(f"Examples dir: {EXAMPLES_DIR}")
    print(f"  Exists: {EXAMPLES_DIR.exists()}")
    if EXAMPLES_DIR.exists():
        plc_files = list(EXAMPLES_DIR.glob("*.plc"))
        print(f"  Found {len(plc_files)} .plc files")

    print(f"\nDocs dir: {DOCS_DIR}")
    print(f"  Exists: {DOCS_DIR.exists()}")
    if DOCS_DIR.exists():
        md_files = list(DOCS_DIR.glob("*.md"))
        print(f"  Found {len(md_files)} .md files")

    print(f"\nSkill file: {SKILL_PATH}")
    print(f"  Exists: {SKILL_PATH.exists()}")

    return EXAMPLES_DIR.exists() and DOCS_DIR.exists() and SKILL_PATH.exists()


def test_server_import():
    """Test server module import"""
    print("\n=== Testing Server Import ===")

    try:
        from server import mcp
        print(f"[PASS] Server imported successfully")
        print(f"   Server name: {mcp.name}")
        return True
    except Exception as e:
        print(f"[FAIL] Server import failed: {e}")
        return False


def main():
    print("RustPLC MCP Server - Functionality Test\n")

    results = {
        "Server Import": test_server_import(),
        "Resources": test_resources(),
        "Rust Bridge": test_rust_bridge(),
    }

    print("\n" + "=" * 50)
    print("Test Summary:")
    for name, passed in results.items():
        status = "[PASS]" if passed else "[FAIL]"
        print(f"  {name}: {status}")

    all_passed = all(results.values())
    print("\n" + ("=" * 50))
    if all_passed:
        print("All tests passed! MCP server is ready.")
        return 0
    else:
        print("Some tests failed. Please check the output above.")
        return 1


if __name__ == "__main__":
    sys.exit(main())
