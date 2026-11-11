//! `hocus perf day [--date YYYY-MM-DD] [--json]`: a day of the desktop's
//! CPU and memory, from the files its recorder writes
//! (`$XDG_STATE_HOME/hocuspanda/perf`, docs/performance.md): its totals,
//! its peaks with what was happening, its signals. In the messages'
//! language, or as JSON.

use std::process::ExitCode;

use hocus_perf::{Day, files, report::report, text};
use jiff::{Zoned, civil::Date, tz::TimeZone};

pub fn day(date: Option<&str>, json: bool) -> ExitCode {
    let tz = TimeZone::system();
    let date = match date {
        Some(said) => match said.parse::<Date>() {
            Ok(date) => date,
            Err(err) => {
                eprintln!("hocus: {said} is not a date (YYYY-MM-DD): {err}");
                return ExitCode::from(2);
            }
        },
        None => Zoned::now().with_time_zone(tz.clone()).date(),
    };
    let Some(dir) = files::dir() else {
        eprintln!("hocus: no state folder ($XDG_STATE_HOME or $HOME)");
        return ExitCode::FAILURE;
    };
    let day = match Day::read(&dir, date) {
        Ok(day) => day,
        Err(err) => {
            eprintln!("hocus: {}: {err}", files::path(&dir, date).display());
            return ExitCode::FAILURE;
        }
    };
    let report = report(&day);
    if json {
        let mut value = match serde_json::to_value(&report) {
            Ok(value) => value,
            Err(err) => {
                eprintln!("hocus: {err}");
                return ExitCode::FAILURE;
            }
        };
        if let Some(report) = value.as_object_mut() {
            report.insert("date".to_owned(), date.to_string().into());
        }
        println!("{value:#}");
    } else {
        print!("{}", text::day(&report, date, text::Lang::from_env(), &tz));
    }
    if day.samples.is_empty() {
        eprintln!(
            "hocus: nothing recorded on {date} in {} ([perf] record, 14 days kept)",
            dir.display()
        );
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
