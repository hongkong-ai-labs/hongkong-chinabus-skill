---
name: hongkong-chinabus-skill
description: Hong Kong cross-border travel from one Rust CLI and MCP server — live Immigration Department queue times at every land boundary, GoByBus coach schedules and prices to mainland China and Macao, the Marine Department cross-boundary ferry board, East Rail service information, and the official MTR high-speed rail timetable for Shenzhen and Guangzhou. Use for any question about crossing between Hong Kong, Shenzhen, Macao, Zhuhai or the mainland.
license: MIT
---

# Hong Kong cross-border travel

`hkchinabus` answers "how do I get across the boundary, and how busy is it?"
from official Hong Kong government feeds plus GoByBus, the main cross-border
coach booking platform. It needs no API key and no account. It is an
independent, unofficial project.

Use it when the user asks about:

- how long the queue is at Lo Wu, Lok Ma Chau, Shenzhen Bay, HZMB, Huanggang,
  Heung Yuen Wai, Man Kam To or Sha Tau Kok
- a coach from Hong Kong to Shenzhen, Guangzhou, Macao, Zhuhai or another
  mainland city, with departure times and prices
- which Hong Kong boarding points serve a particular mainland or Macao hotel
- ferries between Hong Kong and Macao, Shekou, Zhuhai, Zhongshan or Nansha
- the East Rail to Lo Wu or Lok Ma Chau
- high-speed rail from West Kowloon to Futian, Shenzhen North or Guangzhou

## Before first use

```console
$ bash scripts/ensure-binary.sh
/Users/you/.local/bin/hkchinabus
```

```bash
BIN="$(bash scripts/ensure-binary.sh)"
```

The script checks `$HKCHINABUS_BIN`, `hkchinabus` on `PATH`, then builds with
`cargo build --release --locked`. `scripts/install.sh` installs into
`~/.local/bin`.

### Install the binary directly

`npx skills add` installs the skill files. If the binary is missing, install it
with the release installer or cargo:

```bash
curl -fsSL https://raw.githubusercontent.com/hongkong-ai-labs/hongkong-chinabus-skill/main/scripts/install.sh | sh
# or
cargo install --git https://github.com/hongkong-ai-labs/hongkong-chinabus-skill --locked
```

Register it with any MCP client:

```bash
npx add-mcp "$(command -v hkchinabus) mcp" --name hongkong-chinabus -a claude-code -a codex -a cursor -y
claude mcp add hongkong-chinabus -- hkchinabus mcp
codex mcp add hongkong-chinabus -- hkchinabus mcp
```

`add-mcp` detects the client configuration files in the current project; add
`-g` for a global install. `$(command -v hkchinabus)` keeps the entry correct
when the binary is on `PATH`; substitute an absolute path if your client does
not inherit the shell `PATH`.

## CLI

All times are Hong Kong time (UTC+8). Add `--json` anywhere for
machine-readable output.

```bash
$BIN borders
$BIN borders --point SBC
$BIN borders --point "Lo Wu" --json

$BIN coaches --list                    # busiest boarding points
$BIN coaches --from "China HK City"    # where can I go from here?
$BIN coaches --from "China HK City" --to "MGM Cotai" --date 2026-11-22
$BIN coaches --from 14905 --to "Hong Kong Airport" --json

$BIN ferries
$BIN ferries --terminal cft --port Macao
$BIN ferries --port Shekou --direction departure --json

$BIN rail east-rail
$BIN rail east-rail --station lmc
$BIN rail hsr
$BIN rail hsr --to "Shenzhen North" --limit 12
$BIN rail hsr --to "Guangzhou South" --json
$BIN update --check
```

### Border queues

`borders` reads the Immigration Department mobile app feed, which refreshes
roughly every 15 minutes. It reports resident and visitor queues separately
for arrivals and departures. Bands are:

| Published value | Meaning |
| --- | --- |
| 0 | normal, under about 15 minutes |
| 1 | busy, about 15–30 minutes |
| 2 | very busy, about 30–60 minutes |
| 3 | very busy, over about 60 minutes |
| 99 | closed or not in service |

A point showing 99 outside its opening hours is normal, not an error. Do not
describe 99 as "no queue".

### Coaches (GoByBus)

`--from` accepts a station name or GoByBus id. Names resolve exactly first,
then by substring; an ambiguous name returns the candidate ids instead of
guessing. `--to` is resolved against the destinations actually served from
that origin, so a valid hotel on the other side of the city is not offered if
no coach runs there.

`--date` defaults to today and must be within 60 days. GoByBus normally opens
sales a few weeks ahead; an empty list can simply mean seats are not on sale
yet, not that the route does not exist.

The directory contains 780+ boarding and alighting points across Hong Kong,
Shenzhen, Guangzhou, Macao, Zhuhai and other mainland cities. `--list` shows
the boarding points with the widest choice of destinations. The station
directory is cached for one hour.

Fields in `--json` trips: `route_number`, `from`, `to`, `depart`, `arrive`,
`price`, `currency`, `seats_available`, `coach_type` (`bus` or `car`),
`option_id`. Seat counts and prices are live snapshots and can change between
the query and checkout.

### Ferries

`ferries` parses the Marine Department cross-boundary ferry berthing board for
China Ferry Terminal (CFT) and Macau Ferry Terminal (MFT). Records are
arrivals and departures, not a ticket inventory. Each movement has `time`,
`direction`, `port`, `operator`, `terminal`, `berth` and `status`.

Typical ports include Macao, Macao (Taipa), Shekou, Zhuhai, Zhongshan and Nan
Sha. The board is cached for 90 seconds. Outside service hours the board can be
empty or still show the previous day's final sailings; check the timestamp.

### Rail

East Rail information here is reference data. It covers Lo Wu (`low`) and Lok
Ma Chau (`lmc`), the interchange at Sheung Shui, approximate first and last
trains, and headways. MTR includes East Rail in its live Next Train API again
(verified 22 November 2026); use the `hongkong-mtr-skill` `mtr_next_trains`
tool for live departures. If MTR disables the EAL feed again (`NT-205`), quote
the published headway instead of inventing a train.

High-speed rail is different: `rail hsr` reads MTR's own timetable JSON and
returns the actual scheduled departures from West Kowloon, with train number,
departure and arrival times, journey time and intermediate stops. Filter with
`--to` (station code, English name or Chinese name) and cap the table with
`--limit`; `--json` returns every matching departure and its stops. The
source's effective dates are printed with the table.

These are scheduled times, not live running or seat availability. 12306 is the
booking authority, so check it for tickets and disruption before travelling.
Fares and seat counts are deliberately not invented here.

## MCP

```bash
$BIN mcp
```

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

Tools:

| Tool | Arguments |
| --- | --- |
| `border_wait_times` | `{ "point"?: string }` |
| `search_cross_border_coaches` | `{ "from": string, "to": string, "date"?: "YYYY-MM-DD", "limit"?: number }` |
| `cross_border_ferry_board` | `{ "port"?: string, "terminal"?: "cft"\|"mft", "direction"?: "arrival"\|"departure" }` |
| `cross_border_rail_info` | `{ "mode": "east-rail"\|"hsr", "destination"?: string, "limit"?: number }` |

The server speaks JSON-RPC 2.0 over stdio, one message per line, and implements
`initialize`, `ping`, `tools/list` and `tools/call`.

## Cautions

- Queue minutes are Immigration Department bands, not a stopwatch. A coach
  arrival at the boundary can change the picture within minutes.
- Having a coach ticket does not guarantee a seat on a earlier sailing, and
  boundary queues are separate from the coach journey.
- Visa and entry requirements are outside this skill. Check the mainland,
  Macao and Hong Kong immigration authorities before travel.
- High-speed rail times are the published schedule, not live running or seat
  availability. For tickets and disruption, use 12306 or the MTR app.
- Do not present this as an official channel of ImmD, MTR, GoByBus or the
  Marine Department.

## Sources

- Immigration Department queue times:
  `https://secure1.info.gov.hk/immd/mobileapps/2bb9ae17/data/CPQueueTimeR.json`
  and `CPQueueTimeV.json`
- GoByBus: `https://www.gobybus.hk/` route directory and
  `POST https://www.gobybus.hk/zh/getSchedule.php`
- Marine Department cross-boundary ferries:
  `https://crossboundaryferryservices.mardep.gov.hk/en/`
- MTR high-speed rail timetable:
  `https://www.highspeed.mtr.com.hk/res/content/app/XRL_content_Timetable.json`
  (human page: `https://www.highspeed.mtr.com.hk/en/ticket/timetable.html`)
- MTR high-speed rail station names:
  `https://www.highspeed.mtr.com.hk/res/content/XRL_content_TripPlanning.json`
