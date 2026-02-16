#!/usr/bin/env python3
import argparse
import datetime as dt
import html
import json
from pathlib import Path
from typing import Any, Dict, Optional


def load_json(path: Optional[Path]) -> Optional[Dict[str, Any]]:
    if path is None:
        return None
    if not path.exists():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def fmt_event(event: Optional[Dict[str, Any]]) -> str:
    if event is None:
        return "—"
    return (
        f"tick={event.get('tick')} task={event.get('task')} "
        f"{event.get('from_step')}->{event.get('to_step')} "
        f"reason={event.get('reason')}"
    )


def render_html(
    title: str,
    diff: Dict[str, Any],
    summary: Optional[Dict[str, Any]],
    meta: Optional[Dict[str, Any]],
    sil_trace: Optional[str],
    board_trace: Optional[str],
    board_log: Optional[str],
) -> str:
    status_ok = bool(diff.get("is_match"))
    status_label = "PASS" if status_ok else "FAIL"
    status_color = "#117a37" if status_ok else "#9b1c1c"

    context_rows = diff.get("context", [])
    context_html = []
    for row in context_rows:
        idx = row.get("index")
        sil = html.escape(fmt_event(row.get("sil")))
        board = html.escape(fmt_event(row.get("board")))
        context_html.append(
            f"<tr><td>{idx}</td><td><code>{sil}</code></td><td><code>{board}</code></td></tr>"
        )
    context_table = "\n".join(context_html) if context_html else (
        "<tr><td colspan='3'>No mismatch context (trace matched).</td></tr>"
    )

    summary_block = ""
    if summary:
        summary_block = f"""
<h2>Sim-Regress Summary (Optional)</h2>
<ul>
  <li>total: {summary.get("total")}</li>
  <li>pass: {summary.get("pass")}</li>
  <li>fail: {summary.get("fail")}</li>
  <li>failures: {len(summary.get("failures", []))}</li>
</ul>
"""

    meta_items = []
    if meta:
        for k in ("git_commit", "git_status_clean", "ts_unix"):
            if k in meta:
                meta_items.append(f"<li>{k}: <code>{html.escape(str(meta[k]))}</code></li>")
    if sil_trace:
        meta_items.append(f"<li>sil_trace: <code>{html.escape(sil_trace)}</code></li>")
    if board_trace:
        meta_items.append(f"<li>board_trace: <code>{html.escape(board_trace)}</code></li>")
    if board_log:
        meta_items.append(f"<li>board_log: <code>{html.escape(board_log)}</code></li>")
    meta_block = ""
    if meta_items:
        meta_block = "<h2>Inputs & Metadata</h2><ul>" + "".join(meta_items) + "</ul>"

    generated_at = dt.datetime.now(dt.timezone.utc).isoformat()
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{html.escape(title)}</title>
  <style>
    body {{
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Arial, sans-serif;
      margin: 24px;
      color: #222;
    }}
    .badge {{
      display: inline-block;
      padding: 6px 10px;
      border-radius: 8px;
      color: #fff;
      background: {status_color};
      font-weight: 700;
    }}
    code {{
      background: #f4f4f4;
      border-radius: 4px;
      padding: 1px 4px;
      white-space: nowrap;
    }}
    table {{
      border-collapse: collapse;
      width: 100%;
      margin-top: 8px;
    }}
    th, td {{
      border: 1px solid #ddd;
      padding: 8px;
      text-align: left;
      vertical-align: top;
    }}
    th {{
      background: #f7f7f7;
    }}
  </style>
</head>
<body>
  <h1>{html.escape(title)}</h1>
  <p><span class="badge">{status_label}</span></p>
  <ul>
    <li>is_match: {diff.get("is_match")}</li>
    <li>sil_events: {diff.get("sil_events")}</li>
    <li>board_events: {diff.get("board_events")}</li>
    <li>first_mismatch_tick: {diff.get("first_mismatch_tick")}</li>
    <li>mismatch_type: {diff.get("mismatch_type")}</li>
    <li>mismatch_index: {diff.get("mismatch_index")}</li>
  </ul>
  {meta_block}
  {summary_block}
  <h2>Mismatch Context</h2>
  <table>
    <thead>
      <tr><th>Index</th><th>SIL</th><th>Board</th></tr>
    </thead>
    <tbody>
      {context_table}
    </tbody>
  </table>
  <p style="margin-top:20px;color:#666;">Generated at {generated_at}</p>
</body>
</html>
"""


def main() -> int:
    p = argparse.ArgumentParser(
        description="Render a lightweight HTML dashboard from trace-diff JSON."
    )
    p.add_argument("--diff", required=True, help="Path to diff_report.json")
    p.add_argument("--out", required=True, help="Path to output HTML")
    p.add_argument("--title", default="Trace Diff Dashboard")
    p.add_argument("--summary", help="Optional sim-regress summary.json")
    p.add_argument("--meta", help="Optional metadata JSON (e.g. hil_meta.json)")
    p.add_argument("--sil-trace", help="Optional SIL trace path for display")
    p.add_argument("--board-trace", help="Optional board trace path for display")
    p.add_argument("--board-log", help="Optional board log path for display")
    args = p.parse_args()

    diff = load_json(Path(args.diff))
    if diff is None:
        raise SystemExit(f"diff report not found: {args.diff}")
    summary = load_json(Path(args.summary)) if args.summary else None
    meta = load_json(Path(args.meta)) if args.meta else None
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(
        render_html(
            title=args.title,
            diff=diff,
            summary=summary,
            meta=meta,
            sil_trace=args.sil_trace,
            board_trace=args.board_trace,
            board_log=args.board_log,
        ),
        encoding="utf-8",
    )
    print(f"dashboard written: {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

