# Model Context Protocol (MCP) Server

**`vic3-analyzer`** includes a built-in [Model Context Protocol](https://modelcontextprotocol.io/) (MCP) server implemented via [`rmcp`](https://crates.io/crates/rmcp) in `crates/vic3-mcp`.

This enables desktop AI assistants (such as Claude Desktop, Cursor, and custom agent workflows) to connect directly to your Victoria 3 campaign saves, diagnose shortages, evaluate what-if building adjustments, and plan national objectives.

---

## How It Works (Direct Local Integration)

The MCP server runs as a lightweight local helper directly on your machine without requiring network setup, web servers, or open ports:

```bash
# Launch MCP assistant server directly (headless, no window opened)
cargo run -p vic3-analyzer -- mcp
```

| Property | Specification |
| --- | --- |
| **Communication** | Direct local process standard I/O (JSON-RPC) |
| **Logging** | Diagnostic and error logs are emitted to `stderr` without polluting protocol communication |
| **GUI Window** | Runs headlessly in the background when invoked by your AI app |
| **Query Engine** | Embedded [Apache DataFusion](sql.md) instance (`vic3-sql`) |
| **Configuration** | Shares `config.toml` with the Tauri desktop companion |

---

## Automatic Setup (Recommended)

`vic3-cli` can automatically detect installed desktop AI applications on your machine and configure them with a single interactive command:

```bash
# Interactive setup (checkboxes for detected apps):
cargo run -p vic3-cli -- mcp install

# Automated non-interactive install to all detected apps:
cargo run -p vic3-cli -- mcp install -y

# Check detection and integration status:
cargo run -p vic3-cli -- mcp status
```

---

## Manual Client Configuration

If you prefer to configure your AI applications manually, or for applications without direct file access:

### 1. Claude Desktop
* **macOS:** `~/Library/Application Support/Claude/claude_desktop_config.json`
* **Windows:** `%APPDATA%\Claude\claude_desktop_config.json`
* **Linux:** `~/.config/Claude/claude_desktop_config.json`

```json
{
  "mcpServers": {
    "vic3-analyzer": {
      "command": "/path/to/vic3-analyzer",
      "args": ["mcp"]
    }
  }
}
```

### 2. LM Studio (Local GPU Models)
* **macOS / Linux:** `~/.lmstudio/mcp.json`
* **Windows:** `%USERPROFILE%\.lmstudio\mcp.json`

```json
{
  "mcpServers": {
    "vic3-analyzer": {
      "command": "/path/to/vic3-analyzer",
      "args": ["mcp"]
    }
  }
}
```

### 3. Windows 11 Copilot (On-Device Registry)
On Windows 11, register with the system AI via `odr.exe`:

```cmd
odr.exe mcp add --name vic3-analyzer --command "C:\path\to\vic3-analyzer.exe" --args "mcp"
```

### 4. OpenAI Codex CLI
* **macOS / Linux / Windows:** `~/.codex/config.toml`

```toml
[mcp_servers.vic3-analyzer]
command = "/path/to/vic3-analyzer"
args = ["mcp"]
```

### 5. Cursor & Claude Code
* **Cursor:** `~/.cursor/mcp.json`
* **Claude Code:** `~/.claude.json`

```json
{
  "mcpServers": {
    "vic3-analyzer": {
      "command": "/path/to/vic3-analyzer",
      "args": ["mcp"]
    }
  }
}
```

> **Tip:** You can generate a copy-paste snippet customized for your system at any time with:
> `vic3-cli mcp show-config --client claude-desktop` (or `--client codex` / `--client windows-copilot`).

---

## Recommended Agent Workflow

1. **Discover Saves:** Use `query` against the `saves` table or read the `vic3://saves` resource.
2. **Bind Session:** Call `use_save` with a save name stub (`"autosave"`) or selector (`"latest_autosave"`).
3. **Inspect Campaign:** Call `campaign_brief` for a compact overview of domestic goods shortages, regional hotspots, and critical alerts.
4. **Deep Dive:** Execute specific SQL queries using `query` or preview economic modifications using `preview_delta`.
5. **Advise:** Call `advice` for what to do next; after a newer save is bound, `check_advice` confirms which suggestions were done.

---

## Tools Reference

### 1. `use_save`
Binds an active save session for subsequent SQL queries and analytical tools.

| Parameter | Type | Required | Description |
| --- | --- | :---: | --- |
| `name` | string | Optional | Save filename stub (e.g. `"autosave"` or `"prussia_1836.v3"`) |
| `selector` | string | Optional | Shorthand selector: `"latest"`, `"latest_autosave"`, or `"latest_named"` |
| `location` | string | Optional | Disambiguate storage root: `"local"` or `"steam_cloud"` |

### 2. `campaign_brief`
Generates a compact summary of the active save without requiring custom SQL queries.
- **Output:** Active country, game date, top domestic shortages, regional bottleneck hotspots, and an alert severity breakdown.

### 3. `query`
Executes read-only DataFusion SQL against the loaded campaign fact tables ([`sql.md`](sql.md)).

| Parameter | Type | Required | Description |
| --- | --- | :---: | --- |
| `sql` | string | Yes | Read-only SQL query (`SELECT ... FROM active.goods ...`) |
| `format` | string | No | `"json"` (default) or `"csv"` |

### 4. `preview_delta`
Simulates the economic impact of building expansions or production method swaps with live price re-equilibration.

| Parameter | Type | Description |
| --- | --- | --- |
| `building` | string | Building type ID (e.g. `"building_rye_farm"`) |
| `extra_levels` | number | Number of levels to simulate adding |
| `delta` | object | Advanced [`WorldDelta`](json-schema.md) object for complex multi-building PM swaps |

### 5. `refresh_catalog`
Rescans allowlisted save directories and updates the available save catalog.

### 6. `explain`
Returns the DataFusion logical and physical execution plan for a SQL statement.

### 7. `advice`
Ranked suggestions for the played country, grouped by domain (same engine as the desktop Advisor tab).

| Parameter | Type | Required | Description |
| --- | --- | :---: | --- |
| `domain` | string | No | `"economy"`, `"laws"` or `"war"`; omit for all |

- **Output:** `{ advice: [{ id, domain, goal?, priority, title, detail, check? }], limitations }`. `priority` 1 is urgent, 2 worthwhile. Economy items carry `goal` (`growth`, `living_standards`, `industrialization`, `self_sufficiency`) and laws items (`economy`, `authority`, `democracy`: the next step per law group with interest-group support from the save); items without `goal` apply to every goal.

### 8. `check_advice`
After binding a newer save, reports which earlier suggestions were carried out.

| Parameter | Type | Required | Description |
| --- | --- | :---: | --- |
| `checks` | array | Yes | `check` objects copied from `advice` items |

- **Output:** `{ done: [bool] }` in the same order.

### 9. `live_status`
The newest in-game line written weekly by the Oikonomia Advisor mod to `logs/debug.log` beside the save folders, newer than any save. Takes `{}`; needs no bound save.

- **Output:** `country`, `date` (in-game, localized), `logged_at` (wall clock), `weekly_balance`, `weekly_income`, `treasury`, `debt_share`, `military_share`, `weeks_to_debt`, `gdp`; `states` (`name`, `unemployed`, `infrastructure_balance`, negative = short) for every owned state and `goods` (`name`, `price_vs_base`, 0.3 = 30% dearer) for goods at least 20% off base price on the home market, both from the same week (newest line only, not kept in `history`); `null` when the mod has not logged yet.

### 10. `history`
Trends over time for the played country as numbers. Optional `{ "points": 24 }` per line (3–200).

- **Output:** `{ charts: [{ key, title, unit, lines: [{ label, points: [[year, value]] }] }] }`: GDP, standard of living, literacy, prestige, population by strata, radicals and loyalists from the save; weekly income, balance, treasury, debt and military share from the mod's log (kept across game sessions in `<app data>/live_history.jsonl`).

### 11. `budget`
The played country's budget straight from the save, no market solve. Takes `{}`.

- **Output:** `risk` (`safe` | `deficit` | `danger` | `critical`), `treasury`, `weekly_balance` / `weekly_income` / `weekly_expenses`, `debt`, `credit_limit`, `weeks_to_debt`, `weeks_to_default`, and `expenses` / `incomes` per budget category with their largest buildings. `war` estimates the budget with the whole army mobilised: `extra_weekly` (mobilised battalions consume twice their goods, `mobilized_battalion_consumption` in 1.13.9), `weekly_balance`, `weeks_to_unhealthy`, `weeks_to_default`; a lower bound without mobilisation options, conscripts or lost trade. Steps name the game's buttons with the Polish game's label in brackets (`playbook/ui_terms.toml`, from the game localization). `null` when the save has no budget.
- `advice` carries the same report as `budget`, plus budget-guard and playbook items; every item may have a `why` explaining the game mechanism.
- Items may also carry `steps` (what to click in the game) and `why_now` (the save's values against the rule's thresholds). `market` lists dear and cheap goods (see `market_doctor`); `outcomes` lists followed suggestions with the budget before and after, measured against `<app data>/advice_log.jsonl`.

### 12. `explain_rule`
One playbook rule for a player who does not understand it or how to do it. Takes `{ "id": "playbook:gen.tax-bridge" }` (prefix optional); works without `use_save`.

- **Output:** `title`, `detail`, `why`, `steps`, `sources`, `patch`, `terms` (Polish glossary entries the rule uses: `pl_name`, `pl`), and with a bound save `applies_now` and `why_now`.

### 13. `market_doctor`
After `use_save`: goods at least 20% off base price, worst first (up to 6 dear and 4 cheap).

- **Output:** `[{ label, price_vs_base, dear, producers, dear_inputs, consumers, yours: [[state, building, levels, profit per level]], advice }]`. `dear_inputs` come from the methods the player's buildings run (or the starting methods): fix those first.

### 14. `glossary`
Polish glossary of the game terms the advisor uses: `[{ key, en, pl_name, pl }]`. Takes `{}`.

### 15. `states`
After `use_save`: the player's states, most populous first: `[{ label, population, workforce, unemployed, peasant_share, literacy, infrastructure, infrastructure_usage, best_build }]`. `unemployed` counts working-age pops without a workplace (peasants and slaves aside), an estimate.

---

## Resources & Prompts

### Available Resources
- `vic3://saves`: Current list of cataloged save games.
- `vic3://session`: Status of the currently bound save and loaded definition blob.
- `vic3://schema`: Database schema listing available tables, columns, and TVFs.
- `vic3://docs/sql`: In-engine documentation of SQL fact tables and functions.

### Built-In Prompts
- `investigate_shortages`: Guides the agent through identifying and resolving domestic supply deficits.
- `compare_latest_autosave`: Summarizes campaign progression since the last save.
- `military_readiness`: Evaluates army and munitions readiness for conflict.
- `plan_research`: Plans an optimal tech rush sequence for a target technology.

---

## Protocol Verification & Smoke Client

The repository includes an in-repo NDJSON protocol verification client:

```bash
# Run automated MCP round-trip against a local save
python3 scripts/mcp_smoke.py --name autosave

# Test with automatic save selector
python3 scripts/mcp_smoke.py --selector latest_autosave --location local
```
