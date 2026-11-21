# hongkong-chinabus-skill

Hong Kong cross-border travel in one Rust CLI and MCP server:

- **Border queues** — live Immigration Department wait times at all nine land
  control points, split by residents and visitors, arrivals and departures.
- **Cross-border coaches** — live GoByBus departures, prices and seat counts
  between Hong Kong and Shenzhen, Guangzhou, Macao, Zhuhai and beyond.
- **Cross-boundary ferries** — the Marine Department berthing board for China
  Ferry Terminal and Macau Ferry Terminal.
- **Rail** — East Rail service to Lo Wu and Lok Ma Chau, and high-speed rail
  destinations and journey times from West Kowloon.

No API keys, no accounts, no scraping of booking pages. Data comes from
official government feeds and GoByBus's public schedule endpoint.

## Install

Requires Rust 1.89 or newer.

```bash
cargo build --release --locked
./scripts/install.sh          # installs into ~/.local/bin
```

Or run from the checkout with `cargo run -- <command>`.

## Usage

```bash
hkchinabus borders
hkchinabus borders --point "Shenzhen Bay"

hkchinabus coaches --list
hkchinabus coaches --from "China HK City"
hkchinabus coaches --from "China HK City" --to "MGM Cotai" --date 2026-11-22
hkchinabus coaches --from 14905 --to "Hong Kong Airport" --json

hkchinabus ferries
hkchinabus ferries --port Macao --terminal mft

hkchinabus rail east-rail --station low
hkchinabus rail hsr --to "Guangzhou South"
```

Add `--json` before the subcommand for structured output:

```bash
hkchinabus --json borders
hkchinabus --json ferries --port Shekou
```

## What the data means

`borders` prints bands rather than minutes because that is what ImmD
publishes: `0` normal (under about 15 minutes), `1` busy (15–30), `2` very busy
(30–60), `3` very busy (over 60), `99` closed or not in service. A `99` before
opening or after closing is expected.

`coaches` resolves station names against GoByBus's own directory. If a name is
shared by several stations it lists the ids rather than guessing. GoByBus
opens sales a few weeks ahead, so an empty result may mean "not on sale yet".
Prices and seat counts are live and can change.

`ferries` shows berthing movements, not ticket availability. The board is
cached for 90 seconds and may still show the last sailings after service ends.

`rail` is reference data. MTR does not publish a working live East Rail feed
(the endpoint returns `NT-205 ERL line is disabled in CMS`), and the high-speed
timetable is a PDF with 12306 as the booking authority. The CLI therefore
reports service patterns and official links, never invented train times.

## MCP

```json
{
  "mcpServers": {
    "hongkong-chinabus": {
      "command": "/absolute/path/to/hkchinabus",
      "args": ["mcp"]
    }
  }
}
```

Tools: `border_wait_times`, `search_cross_border_coaches`,
`cross_border_ferry_board`, `cross_border_rail_info`. See
[`SKILL.md`](SKILL.md) for argument schemas and agent guidance.

## Development

```bash
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

The parsers for the GoByBus directory and the Marine Department board are
covered by unit tests; the live endpoints are exercised manually because they
change during the day.

## Licence

MIT. This is an independent project, not affiliated with the Hong Kong
government, MTR, GoByBus or any ferry operator.
