---
name: hongkong-chinabus-skill
description: Hong Kong cross-border travel from one Rust CLI and MCP server — live Immigration Department queue times at every land boundary, GoByBus coach schedules and prices to mainland China and Macao, the Marine Department cross-boundary ferry board, and East Rail / high-speed rail service information for Lo Wu, Lok Ma Chau, Shenzhen and Guangzhou. Use for any question about crossing between Hong Kong, Shenzhen, Macao, Zhuhai or the mainland.
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
$BIN rail hsr --to "Shenzhen North"
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

Rail information is reference data, not a live feed:

- **East Rail** — Lo Wu (`low`) and Lok Ma Chau (`lmc`), the interchange at
  Sheung Shui, approximate first and last trains, and headways.
- **High-speed rail** — journey times and typical frequency from West Kowloon
  to Futian, Shenzhen North, Guangmingcheng, Dongguannan, Changping, Dongguan,
  Guangzhou Xintang, Guangzhou South and Guangzhou East.

MTR's open-data East Rail endpoint currently returns
`NT-205 ERL line is disabled in CMS`, so there is no honest live next-train
number to quote. MTR publishes the high-speed timetable only as a PDF and
12306 is the booking authority; train numbers, seat availability and fares are
deliberately not invented here. Always point the user at the official
timetable or 12306 for a specific departure.

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
| `cross_border_rail_info` | `{ "mode": "east-rail"\|"hsr", "destination"?: string }` |

The server speaks JSON-RPC 2.0 over stdio, one message per line, and implements
`initialize`, `ping`, `tools/list` and `tools/call`.

## Cautions

- Queue minutes are Immigration Department bands, not a stopwatch. A coach
  arrival at the boundary can change the picture within minutes.
- Having a coach ticket does not guarantee a seat on a earlier sailing, and
  boundary queues are separate from the coach journey.
- Visa and entry requirements are outside this skill. Check the mainland,
  Macao and Hong Kong immigration authorities before travel.
- High-speed rail and East Rail details here are service patterns. For a real
  departure, use the MTR app, the official timetable or 12306.
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
  `https://www.highspeed.mtr.com.hk/en/ticket/timetable.html`
