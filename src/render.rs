//! Human-readable tables and rail reference text.

use crate::api::{BorderReport, CoachStation, CoachTrip, FerryReport};

pub fn borders(report: &BorderReport) -> String {
    let mut out = format!("Hong Kong land boundary queues — {}\n", report.fetched_at);
    out.push_str(
        "Source: Immigration Department mobile app feed (updates about every 15 min).\n\n",
    );
    out.push_str(&format!(
        "{:<40} {:<24} {:<24} {:<24} {}\n",
        "Control point",
        "Resident arrival",
        "Resident departure",
        "Visitor arrival",
        "Visitor departure"
    ));
    out.push_str(&"-".repeat(146));
    out.push('\n');
    for point in &report.points {
        out.push_str(&format!(
            "{:<40} {:<24} {:<24} {:<24} {}\n",
            format!("{} ({})", point.name, point.code),
            point.resident_arrival,
            point.resident_departure,
            point.visitor_arrival,
            point.visitor_departure
        ));
    }
    out.push_str("\n99 means the control point is closed or not in service at this hour.\n");
    out
}

pub fn coach_stations(stations: &[CoachStation], departures: &[(CoachStation, usize)]) -> String {
    let mut out = String::from("GoByBus cross-border coach boarding points\n\n");
    out.push_str(&format!(
        "{:<8} {:<46} {:<10} {}\n",
        "ID", "Station", "Outbound", "Address"
    ));
    out.push_str(&"-".repeat(110));
    out.push('\n');
    for (station, count) in departures {
        out.push_str(&format!(
            "{:<8} {:<46} {:<10} {}\n",
            station.id,
            truncate(&station.name, 45),
            count,
            truncate(&station.address, 60)
        ));
    }
    out.push_str(&format!(
        "\n{} boarding points. Use --from <name or id> to see destinations.\n",
        stations.len()
    ));
    out
}

pub fn coach_destinations(station: &CoachStation, destinations: &[CoachStation]) -> String {
    let mut out = format!(
        "Cross-border coach destinations from {} ({})\n\n",
        station.name, station.id
    );
    for destination in destinations {
        out.push_str(&format!(
            "{:<8} {:<48} {}\n",
            destination.id,
            truncate(&destination.name, 47),
            truncate(&destination.address, 60)
        ));
    }
    out.push_str(&format!(
        "\n{} destinations. Add --to <name or id> and --date YYYY-MM-DD for live times and seats.\n",
        destinations.len()
    ));
    out
}

pub fn coach_trips(
    from: &CoachStation,
    to: &CoachStation,
    date: &str,
    trips: &[CoachTrip],
) -> String {
    let mut out = format!("{} → {} on {}\n", from.name, to.name, date);
    if trips.is_empty() {
        out.push_str("\nNo departures returned. The pair may not run on this date, or seats may not be on sale yet.\n");
        return out;
    }
    out.push_str(&format!(
        "\n{:<10} {:<10} {:<10} {:<8} {:<7} {}\n",
        "Depart", "Arrive", "Price", "Seats", "Type", "Route"
    ));
    out.push_str(&"-".repeat(72));
    out.push('\n');
    for trip in trips {
        out.push_str(&format!(
            "{:<10} {:<10} {:<10} {:<8} {:<7} {}\n",
            short_time(&trip.depart),
            short_time(&trip.arrive),
            format!("{} {}", trip.currency, trip.price),
            trip.seats_available
                .map(|seats| seats.to_string())
                .unwrap_or_else(|| "-".to_string()),
            trip.coach_type,
            trip.route_number
        ));
    }
    out.push_str(&format!(
        "\n{} departures. Prices are per adult as published by GoByBus; availability changes quickly.\n",
        trips.len()
    ));
    out
}

pub fn ferries(report: &FerryReport) -> String {
    let mut out = String::from("Cross-boundary ferry berthing board\n");
    if !report.cft_updated.is_empty() || !report.mft_updated.is_empty() {
        out.push_str(&format!(
            "China Ferry Terminal: {}   Macau Ferry Terminal: {}\n",
            value_or_dash(&report.cft_updated),
            value_or_dash(&report.mft_updated)
        ));
    }
    out.push_str("Source: Marine Department cross-boundary ferry services.\n\n");
    if report.movements.is_empty() {
        out.push_str("No movements published right now (services normally start around 07:00).\n");
        return out;
    }
    out.push_str(&format!(
        "{:<18} {:<10} {:<28} {:<20} {:<5} {}\n",
        "Time", "Direction", "Port", "Operator", "Berth", "Status"
    ));
    out.push_str(&"-".repeat(100));
    out.push('\n');
    for movement in &report.movements {
        out.push_str(&format!(
            "{:<18} {:<10} {:<28} {:<20} {:<5} {}\n",
            movement.time,
            movement.direction,
            truncate(&movement.port, 27),
            truncate(&movement.operator, 19),
            movement.berth,
            movement.status
        ));
    }
    out
}

pub fn east_rail(station: &str) -> String {
    for (slug, name, name_zh, hours, note) in crate::api::rail::EAST_RAIL {
        if station == "all"
            || station.eq_ignore_ascii_case(slug)
            || name.to_lowercase().contains(&station.to_lowercase())
        {
            return format!(
                "East Rail line — {name} ({name_zh})\n\n\
                 Service: {hours} (approximate)\n\
                 Interchange: Sheung Shui, then {name}\n\
                 {note}\n\n\
                 Live next-train data is not offered by MTR's open data feed (the East Rail endpoint is disabled), \
                 so this skill reports the published service pattern instead. Check the MTR app for the next train.\n\
                 Official: https://www.mtr.com.hk/en/customer/services/domestic_timetable.html\n"
            );
        }
    }
    let mut out = String::from("East Rail line cross-boundary termini\n\n");
    for (slug, name, name_zh, hours, note) in crate::api::rail::EAST_RAIL {
        out.push_str(&format!(
            "{name} ({name_zh}) [{slug}]\n  {hours}\n  {note}\n\n"
        ));
    }
    out.push_str("Use --station low or --station lmc for details.\n");
    out
}

pub fn hsr(destination: &str) -> String {
    let mut matches = Vec::new();
    for (slug, name, name_zh, duration, frequency) in crate::api::rail::HSR_DESTINATIONS {
        if destination == "all"
            || destination.eq_ignore_ascii_case(slug)
            || name.to_lowercase().contains(&destination.to_lowercase())
        {
            matches.push((slug, name, name_zh, duration, frequency));
        }
    }
    if matches.is_empty() {
        let mut out = String::from("High-speed rail destinations from West Kowloon\n\n");
        for (_, name, name_zh, duration, frequency) in crate::api::rail::HSR_DESTINATIONS {
            out.push_str(&format!("{name} ({name_zh}) — {duration}, {frequency}\n"));
        }
        out.push_str("\nUse --to <destination> for detail.\n");
        return out;
    }
    let mut out = String::new();
    for (slug, name, name_zh, duration, frequency) in matches {
        out.push_str(&format!(
            "High-speed rail — West Kowloon to {name} ({name_zh}) [{slug}]\n\n\
             Journey time: {duration}\n\
             Frequency: {frequency}\n\
             {}\n\n\
             Official timetable: https://www.highspeed.mtr.com.hk/en/ticket/timetable.html\n\
             Booking and live seat availability: https://www.12306.cn\n",
            crate::api::rail::HSR_NOTE
        ));
    }
    out
}

fn short_time(value: &str) -> String {
    value.get(0..5).unwrap_or(value).to_string()
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }
    let mut out: String = value.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn value_or_dash(value: &str) -> &str {
    if value.is_empty() { "-" } else { value }
}

#[cfg(test)]
mod tests {
    #[test]
    fn truncates_on_char_boundaries() {
        assert_eq!(super::truncate("深圳灣口岸", 3), "深圳…");
        assert_eq!(super::short_time("08:05:00"), "08:05");
    }
}
