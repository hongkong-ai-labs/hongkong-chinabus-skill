//! `hkchinabus` — Hong Kong cross-border coaches, ferries, rail and border queues.

mod api;
mod mcp;
mod render;
mod sources;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use clap::{Parser, Subcommand};

use api::{Client, CoachStation};

mod update;

#[derive(Parser)]
#[command(
    name = "hkchinabus",
    version,
    about = "Hong Kong cross-border travel: border queues, coaches, ferries, East Rail and high-speed rail"
)]
struct Cli {
    /// Print machine-readable JSON instead of tables.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Live immigration queue times at every land boundary.
    Borders {
        /// Filter by control point name or code, e.g. Shenzhen Bay or SBC.
        #[arg(long)]
        point: Option<String>,
    },
    /// GoByBus cross-border coach stations and live departures.
    Coaches {
        /// Boarding point name or GoByBus station id.
        #[arg(long)]
        from: Option<String>,
        /// Alighting point name or GoByBus station id.
        #[arg(long)]
        to: Option<String>,
        /// Travel date, YYYY-MM-DD (Hong Kong time). Defaults to today.
        #[arg(long)]
        date: Option<String>,
        /// List boarding points instead of a route.
        #[arg(long)]
        list: bool,
        /// Maximum rows to print.
        #[arg(long, default_value_t = 40)]
        limit: usize,
    },
    /// Live cross-boundary ferry berthing board.
    Ferries {
        /// Filter by port or operator, e.g. Macao, Shekou, Zhuhai.
        #[arg(long)]
        port: Option<String>,
        /// Terminal: cft (China Ferry Terminal) or mft (Macau Ferry Terminal).
        #[arg(long)]
        terminal: Option<String>,
        /// arrival or departure.
        #[arg(long)]
        direction: Option<String>,
    },
    /// East Rail cross-boundary termini and high-speed rail destinations.
    Rail {
        #[command(subcommand)]
        mode: RailMode,
    },
    /// Update this binary using the method it was installed with
    Update {
        /// Only report the current version, latest release and install method
        #[arg(long)]
        check: bool,
        /// Reinstall even when the current version is up to date
        #[arg(long)]
        force: bool,
    },
    /// Run the MCP server over stdio.
    Mcp,
}

#[derive(Subcommand)]
enum RailMode {
    /// Lo Wu and Lok Ma Chau service pattern.
    EastRail {
        /// low, lmc, or all.
        #[arg(long, default_value = "all")]
        station: String,
    },
    /// West Kowloon high-speed rail destinations.
    Hsr {
        /// Destination slug or name, or all.
        #[arg(long, default_value = "all")]
        to: String,
        /// Maximum departures to print.
        #[arg(long, default_value_t = 40)]
        limit: usize,
    },
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let client = Client::new()?;
    match cli.command {
        Command::Update { check, force } => {
            update::run(check, force)?;
        }
        Command::Mcp => mcp::serve(client).await?,
        Command::Borders { point } => {
            let mut report = client.borders().await?;
            if let Some(filter) = point {
                let needle = filter.to_lowercase();
                report.points.retain(|item| {
                    item.code.to_lowercase() == needle
                        || item.name.to_lowercase().contains(&needle)
                        || item.name_zh.contains(&filter)
                });
                if report.points.is_empty() {
                    bail!("no control point matches {filter:?}");
                }
            }
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", render::borders(&report));
            }
        }
        Command::Coaches {
            from,
            to,
            date,
            list,
            limit,
        } => coaches(&client, cli.json, from, to, date, list, limit).await?,
        Command::Ferries {
            port,
            terminal,
            direction,
        } => {
            let mut report = client.ferries().await?;
            if let Some(filter) = port {
                let needle = filter.to_lowercase();
                report.movements.retain(|item| {
                    item.port.to_lowercase().contains(&needle)
                        || item.operator.to_lowercase().contains(&needle)
                });
            }
            if let Some(filter) = terminal {
                let needle = filter.to_lowercase();
                let code = match needle.as_str() {
                    "cft" | "china" => "CFT".to_string(),
                    "mft" | "macau" | "macao" => "MFT".to_string(),
                    other => other.to_uppercase(),
                };
                report
                    .movements
                    .retain(|item| item.terminal.eq_ignore_ascii_case(&code));
            }
            if let Some(filter) = direction {
                report.movements.retain(|item| {
                    item.direction
                        .to_lowercase()
                        .starts_with(&filter.to_lowercase())
                });
            }
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", render::ferries(&report));
            }
        }
        Command::Rail { mode } => match mode {
            RailMode::EastRail { station } => print!("{}", render::east_rail(&station)),
            RailMode::Hsr { to, limit } => {
                let report = client.hsr_timetable(&to).await?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    print!("{}", render::hsr_timetable(&report, limit));
                }
            }
        },
    }
    Ok(())
}

async fn coaches(
    client: &Client,
    json: bool,
    from: Option<String>,
    to: Option<String>,
    date: Option<String>,
    list: bool,
    limit: usize,
) -> Result<()> {
    let (stations, adjacency) = client.coach_directory().await?;
    if stations.is_empty() {
        bail!("GoByBus returned no coach stations");
    }
    let Some(from) = from.filter(|_| !list) else {
        let mut departures: Vec<(CoachStation, usize)> = stations
            .iter()
            .filter(|station| adjacency.contains_key(&station.id))
            .map(|station| {
                (
                    station.clone(),
                    adjacency.get(&station.id).map(Vec::len).unwrap_or(0),
                )
            })
            .collect();
        departures.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.name.cmp(&right.0.name)));
        departures.truncate(limit);
        if json {
            let payload: Vec<_> = departures
                .iter()
                .map(|(station, count)| {
                    serde_json::json!({"id": station.id, "name": station.name, "address": station.address, "destinations": count})
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&payload)?);
        } else {
            print!("{}", render::coach_stations(&stations, &departures));
        }
        return Ok(());
    };

    let origin = resolve(&stations, &adjacency, &from)?;
    let destination_ids = adjacency.get(&origin.id).with_context(|| {
        format!(
            "{} ({}) has no cross-border departures in GoByBus",
            origin.name, origin.id
        )
    })?;
    let mut destinations: Vec<CoachStation> = destination_ids
        .iter()
        .filter_map(|id| stations.iter().find(|station| &station.id == id).cloned())
        .collect();
    destinations.sort_by(|left, right| left.name.cmp(&right.name));

    let Some(to) = to else {
        if json {
            println!("{}", serde_json::to_string_pretty(&destinations)?);
        } else {
            print!("{}", render::coach_destinations(&origin, &destinations));
        }
        return Ok(());
    };

    let arrival = resolve(&destinations, &adjacency, &to)?;
    let day = match date {
        Some(value) => NaiveDate::parse_from_str(&value, "%Y-%m-%d")
            .with_context(|| format!("invalid date {value:?}, use YYYY-MM-DD"))?,
        None => api::today_hkt(),
    };
    let today = api::today_hkt();
    if day < today {
        bail!("{day} is in the past (Hong Kong time)");
    }
    if (day - today).num_days() > 60 {
        bail!("GoByBus only sells close to departure; pick a date within 60 days");
    }
    let mut trips = client.coach_trips(&origin.id, &arrival.id, day).await?;
    trips.sort_by(|left, right| left.depart.cmp(&right.depart));
    trips.truncate(limit);
    if json {
        println!("{}", serde_json::to_string_pretty(&trips)?);
    } else {
        print!(
            "{}",
            render::coach_trips(&origin, &arrival, &day.to_string(), &trips)
        );
    }
    Ok(())
}

fn resolve(
    stations: &[CoachStation],
    adjacency: &std::collections::HashMap<String, Vec<String>>,
    query: &str,
) -> Result<CoachStation> {
    if let Some(station) = stations.iter().find(|station| station.id == query) {
        return Ok(station.clone());
    }
    let needle = query.to_lowercase();
    let exact: Vec<&CoachStation> = stations
        .iter()
        .filter(|station| station.name.to_lowercase() == needle)
        .collect();
    if !exact.is_empty() {
        return Ok(exact
            .into_iter()
            .max_by_key(|station| adjacency.get(&station.id).map(Vec::len).unwrap_or(0))
            .cloned()
            .expect("exact match is not empty"));
    }
    let mut matches: Vec<&CoachStation> = stations
        .iter()
        .filter(|station| {
            station.name.to_lowercase().contains(&needle)
                || station.address.to_lowercase().contains(&needle)
        })
        .collect();
    matches.sort_by_key(|station| {
        (
            !station.name.to_lowercase().starts_with(&needle),
            station.name.len(),
        )
    });
    match matches.len() {
        0 => bail!("no GoByBus station matches {query:?}"),
        1 => Ok(matches[0].clone()),
        _ => {
            let options: Vec<String> = matches
                .iter()
                .take(8)
                .map(|station| format!("{} ({})", station.name, station.id))
                .collect();
            bail!(
                "{query:?} matches several stations: {}. Use a station id.",
                options.join(", ")
            );
        }
    }
}
