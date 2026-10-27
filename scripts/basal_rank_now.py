"""Zero-shot experiment: rank the Advisor's candidates with a basal typed-decision model.

Reads MCP `advice` responses (one per save) from a probe transcript, asks a running
basal server (`/v1/systemone`) which decision the player should take first, and
compares the model's order with the app's Now order.

    python -I scripts/basal_rank_now.py tmp/exp/mcp.out http://127.0.0.1:8765 > tmp/exp/report.md
"""

import json
import sys
import time
import urllib.request

# Default goals of the Advisor tabs (economy: growth, laws: economy), as the Now tab uses them.
GOALS = {"economy": "growth", "laws": "economy"}
NOW = 5


def in_view(item):
    goal = item.get("goal")
    return goal is None or (item["domain"] != "war" and goal == GOALS[item["domain"]])


def app_order(advice):
    """The Now order: advice arrives sorted; the budget verdict is lifted to the top."""
    items = [a for a in advice if in_view(a)]
    return sorted(items, key=lambda a: 0 if a["id"] == "budget:balance" else 1)


def money(x):
    a = abs(x)
    s = "-" if x < 0 else ""
    return f"{s}{a / 1e6:.1f}M" if a >= 1e6 else f"{s}{a / 1e3:.1f}K" if a >= 1e3 else f"{x:.0f}"


def state_text(save, result):
    b = result.get("budget") or {}
    lines = [f"Victoria 3 campaign, save {save}. The player is new to the game and keeps going bankrupt."]
    if b:
        lines.append(
            f"Budget: risk {b['risk']}, weekly balance {money(b.get('weekly_balance') or 0)}, "
            f"income {money(b.get('weekly_income') or 0)}, expenses {money(b.get('weekly_expenses') or 0)}, "
            f"treasury {money(b['treasury'])}, debt {money(b['debt'])}, "
            f"treasury lasts {b.get('weeks_to_debt') or 0:.0f} weeks."
        )
        top = ", ".join(f"{g['label']} {money(g['amount'])} ({g['share'] * 100:.0f}%)" for g in b["expenses"][:4])
        lines.append(f"Biggest expenses: {top}.")
    return "\n".join(lines)


def ask(url, state, items):
    criteria = {str(i): f"{a['title']} — {a['detail'][:140]}" for i, a in enumerate(items)}
    body = {
        "state": state,
        "questions": {
            "first": {
                "type": "choice",
                "instructions": "Which ONE of these decisions should the player take first, right now, to avoid bankruptcy and grow the economy?",
                "criteria": criteria,
            }
        },
    }
    req = urllib.request.Request(
        f"{url}/v1/systemone", data=json.dumps(body).encode(), headers={"Content-Type": "application/json"}
    )
    t = time.time()
    with urllib.request.urlopen(req, timeout=600) as resp:
        answer = json.load(resp)["answers"]["first"]
    return answer["probabilities"], time.time() - t


def spearman(a, b):
    n = len(a)
    if n < 2:
        return float("nan")
    rb = {x: i for i, x in enumerate(b)}
    d2 = sum((i - rb[x]) ** 2 for i, x in enumerate(a))
    return 1 - 6 * d2 / (n * (n * n - 1))


def main(transcript, url):
    sys.stdout.reconfigure(encoding="utf-8")
    saves, results = {}, {}
    for line in open(transcript, encoding="utf-8"):
        msg = json.loads(line)
        if "result" not in msg or "content" not in msg.get("result", {}):
            continue
        text = msg["result"]["content"][0]["text"]
        if msg["id"] % 10 == 0:
            saves[msg["id"] // 10] = json.loads(text).get("name", "?")
        elif msg["id"] % 10 == 1:
            results[msg["id"] // 10] = json.loads(text)

    print("# basal-2.0-mini zero-shot: ranking the Now list\n")
    for key in sorted(results):
        save, result = saves.get(key, "?"), results[key]
        items = app_order(result["advice"])
        probs, secs = ask(url, state_text(save, result), items)
        model = sorted(range(len(items)), key=lambda i: -probs[str(i)])
        app = list(range(len(items)))
        overlap = len(set(model[:NOW]) & set(app[:NOW]))
        print(f"## {save}: {len(items)} candidates, {secs:.1f} s on CPU")
        print(f"Top-{NOW} overlap with the app: {overlap}/{NOW}; Spearman over all: {spearman(model, app):.2f}\n")
        print("| # | App Now | Model (probability) |")
        print("|---|---|---|")
        for r in range(NOW):
            a, m = items[app[r]], items[model[r]]
            print(f"| {r + 1} | {a['title']} | {m['title']} ({probs[str(model[r])]:.2f}) |")
        print()


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2].rstrip("/"))
