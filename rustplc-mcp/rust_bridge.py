"""
Rust compiler bridge - calls the rustplc binary via subprocess.
"""

import subprocess
import os
import tempfile
from pathlib import Path


def find_rustplc_binary() -> str:
    """Find the rustplc binary. Checks env var first, then common locations."""
    env_path = os.environ.get("RUSTPLC_PATH")
    if env_path and Path(env_path).exists():
        return env_path

    # Try cargo-built binary relative to this file (.exe first for Windows)
    repo_root = Path(__file__).parent.parent
    candidates = [
        repo_root / "target" / "release" / "rust_plc.exe",
        repo_root / "target" / "release" / "rust_plc",
        repo_root / "target" / "debug" / "rust_plc.exe",
        repo_root / "target" / "debug" / "rust_plc",
    ]
    for c in candidates:
        if c.exists():
            return str(c)

    # Fall back to PATH
    return "rust_plc"


RUSTPLC_BIN = find_rustplc_binary()
REPO_ROOT = Path(__file__).parent.parent


def validate_plc_content(plc_content: str) -> dict:
    """
    Write plc_content to a temp file, run the compiler, return structured result.
    Returns: {"success": bool, "stdout": str, "stderr": str, "report": str}
    """
    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".plc", delete=False, encoding="utf-8"
    ) as f:
        f.write(plc_content)
        tmp_path = f.name

    try:
        result = subprocess.run(
            [RUSTPLC_BIN, tmp_path, "--no-print-ir"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=30,
        )
        return {
            "success": result.returncode == 0,
            "stdout": result.stdout,
            "stderr": result.stderr,
            "report": result.stderr if result.returncode == 0 else result.stdout + result.stderr,
        }
    except FileNotFoundError:
        return {
            "success": False,
            "stdout": "",
            "stderr": "",
            "report": (
                f"rustplc binary not found at '{RUSTPLC_BIN}'. "
                "Set RUSTPLC_PATH env var or build with `cargo build --release`."
            ),
        }
    except subprocess.TimeoutExpired:
        return {
            "success": False,
            "stdout": "",
            "stderr": "",
            "report": "Compilation timed out after 30 seconds.",
        }
    finally:
        os.unlink(tmp_path)


def validate_plc_file(plc_path: str) -> dict:
    """Run the compiler on an existing file path."""
    try:
        result = subprocess.run(
            [RUSTPLC_BIN, plc_path, "--no-print-ir"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=30,
        )
        return {
            "success": result.returncode == 0,
            "stdout": result.stdout,
            "stderr": result.stderr,
            "report": result.stderr if result.returncode == 0 else result.stdout + result.stderr,
        }
    except FileNotFoundError:
        return {
            "success": False,
            "stdout": "",
            "stderr": "",
            "report": f"rustplc binary not found at '{RUSTPLC_BIN}'.",
        }
    except subprocess.TimeoutExpired:
        return {
            "success": False,
            "stdout": "",
            "stderr": "",
            "report": "Compilation timed out after 30 seconds.",
        }


def get_ir_json(plc_content: str) -> dict:
    """Compile and return the IR JSON (stdout) along with verification report (stderr)."""
    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".plc", delete=False, encoding="utf-8"
    ) as f:
        f.write(plc_content)
        tmp_path = f.name

    try:
        result = subprocess.run(
            [RUSTPLC_BIN, tmp_path],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=30,
        )
        return {
            "success": result.returncode == 0,
            "ir_json": result.stdout,
            "report": result.stderr,
        }
    except FileNotFoundError:
        return {
            "success": False,
            "ir_json": "",
            "report": f"rustplc binary not found at '{RUSTPLC_BIN}'.",
        }
    finally:
        os.unlink(tmp_path)
