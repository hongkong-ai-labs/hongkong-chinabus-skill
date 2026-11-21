# hongkong-chinabus-skill

Hong Kong cross-border travel in one Rust CLI and MCP server:

- **Border queues** — live Immigration Department wait times at all nine land
  control points, split by residents and visitors, arrivals and departures.
- **Cross-border coaches** — live GoByBus departures, prices and seat counts
  between Hong Kong and Shenzhen, Guangzhou, Macao, Zhuhai and beyond.
- **Cross-boundary ferries** — the Marine Department berthing board for China
  Ferry Terminal and Macau Ferry Terminal.
- **Rail** — East Rail service to Lo Wu and Lok Ma Chau, and the official MTR
  high-speed rail timetable from West Kowloon with exact scheduled departures.

No API keys, no accounts, no scraping of booking pages. Data comes from
official government feeds and GoByBus's public schedule endpoint.

## Install

**Agent skill** — installs `SKILL.md` and the binary bootstrap script:

```bash
npx skills add hongkong-ai-labs/hongkong-chinabus-skill
npx skills update        # refresh the installed skill later
```

**Prebuilt binary** — macOS (Intel or Apple silicon) and Linux (x86_64 or
aarch64, static musl), checksum-verified, into `~/.local/bin`:

```bash
curl -fsSL https://raw.githubusercontent.com/hongkong-ai-labs/hongkong-chinabus-skill/main/scripts/install.sh | sh
```

**From source** — Rust 1.89 or newer:

```bash
git clone https://github.com/hongkong-ai-labs/hongkong-chinabus-skill
cd hongkong-chinabus-skill
cargo install --path . --locked
# or directly from GitHub:
cargo install --git https://github.com/hongkong-ai-labs/hongkong-chinabus-skill --locked
# or: cargo build --release --locked   # target/release/hkchinabus
```

Verify:

```console
$ hkchinabus --version
```

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
hkchinabus rail hsr --to "Guangzhou South" --limit 12
hkchinabus rail hsr --to "Shenzhen North" --json
hkchinabus update --check
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

East Rail here is reference data. MTR's live Next Train API includes East Rail
again (verified 22 November 2026), so use `mtr_next_trains` from
`hongkong-mtr-skill` for live EAL departures. If MTR disables the EAL feed
(`NT-205`), the CLI reports the service pattern rather than inventing a train.

`rail hsr` reads MTR's own timetable JSON and prints the actual scheduled
departures from West Kowloon: train number, departure, arrival, duration and
intermediate stops. `--to` accepts a station code, English or Chinese name;
`--limit` caps the table; `--json` returns every match. These are scheduled
times, not live running or seat availability. 12306 remains the booking
authority, and fares/seat counts are not invented.

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


Tools: `border_wait_times`, `search_cross_border_coaches`,
`cross_border_ferry_board`, `cross_border_rail_info`. See
[`SKILL.md`](SKILL.md) for argument schemas and agent guidance.

## Run modes and deployment

| Mode | Start with | Use for |
| --- | --- | --- |
| CLI | `hkchinabus <command>` | Interactive use and shell scripts |
| JSON | `<command>` with `--json` | `jq`, automation, other programs |
| MCP stdio | `hkchinabus mcp` | Desktop and CLI MCP clients |
| Container | `docker run --rm -i hkchinabus mcp` | No local Rust toolchain |

### MCP and authentication

This server is **stdio-only**: the client launches the process and speaks MCP
over stdin/stdout. There is no listening port, so there is no network
authentication layer and no API-key setting. Optional API keys in this project
family apply only to servers that expose an HTTP transport. Keep the process
local and let the MCP client manage it.

Typical client entry:

```json
{
  "mcpServers": {
    "hongkong-chinabus-skill": {
      "command": "hkchinabus",
      "args": ["mcp"]
    }
  }
}
```

If the client does not inherit `PATH`, use the absolute installed path.

### Docker and Apple container

```bash
docker build -t hkchinabus .
docker run --rm hkchinabus --help
docker run --rm -i hkchinabus mcp          # stdio MCP
docker compose run --rm stdio             # same, through Compose
```

Apple's `container` runtime uses the same OCI image:

```bash
container build -t hkchinabus .
container run --rm hkchinabus --help
container run --rm -i hkchinabus mcp
```

The runtime image contains the release binary and CA certificates, runs as a
non-root user, and defaults to `--help` so a bare `docker run` is harmless.

### Cloudflare and WASM

There is no Worker or WASM deployment. The binary talks directly to upstream
HTTP APIs over TLS and uses a Tokio runtime; packaging it as a stdio server is
the supported integration. A Worker would need a separate HTTP/JavaScript
adapter and is out of scope for this repo.

### Releases

Pushing a `v*` tag runs the release workflow, which checks the tag against
`Cargo.toml` and `CHANGELOG.md`, builds four targets, smoke-tests native
binaries and publishes:

- `hkchinabus-<tag>-<target>.tar.gz` for macOS and static Linux
- `SHA256SUMS` covering every tarball

`scripts/install.sh` downloads the latest release, verifies its checksum and
falls back to `cargo build --release --locked` when no release is available.
`just tag X.Y.Z` creates the annotated tag locally; `just release X.Y.Z` tags
and pushes to trigger the workflow.

## Development

```bash
just                 # fmt check, clippy with -D warnings, tests
just skill-check     # validate SKILL.md and shell syntax
just container       # build the image and print CLI help
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

`rust-toolchain.toml` pins the stable toolchain with `rustfmt` and
`clippy`, so `rustup` sets contributors up automatically. `just doc`
builds the API documentation. See `CONTRIBUTING.md` for the full check
list and the signed-commit requirement.


The parsers for the GoByBus directory and the Marine Department board are
covered by unit tests; the live endpoints are exercised manually because they
change during the day.

## Licence

MIT. This is an independent project, not affiliated with the Hong Kong
government, MTR, GoByBus or any ferry operator.
