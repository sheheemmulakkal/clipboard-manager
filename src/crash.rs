//! Keep the daemon alive when a GTK callback panics, and record why.
//!
//! A panic that unwinds into GTK's C code aborts the whole process, so
//! callbacks that do real work run inside [`guarded`]. Every panic (caught
//! or not) is appended to `crash.log` by the hook from [`install_hook`].

use std::any::Any;
use std::io::Write;
use std::panic::{self, AssertUnwindSafe};

const CRASH_LOG_MAX_BYTES: u64 = 256 * 1024;

/// Run `f`; if it panics, log it and return `false` instead of unwinding
/// into the caller (GTK).
pub fn guarded(what: &str, f: impl FnOnce()) -> bool {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(()) => true,
        Err(payload) => {
            tracing::error!("[crash] {what} panicked (recovered): {}", payload_message(payload.as_ref()));
            false
        }
    }
}

fn payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".into()
    }
}

/// One crash.log entry.
pub fn crash_record(time: &str, context: &str, location: Option<&str>, message: &str) -> String {
    format!(
        "{time}  [{context}] panicked at {}: {message}\n",
        location.unwrap_or("<unknown>")
    )
}

/// Append every panic (message, location, backtrace) to
/// `$XDG_STATE_HOME/clipboard-manager/crash.log`, then run the default hook.
pub fn install_hook() {
    let default = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".into());
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let mut record = crash_record(&utc_now(), &format!("thread {thread}"), location.as_deref(), &message);
        record.push_str(&format!("{}\n", std::backtrace::Backtrace::force_capture()));
        append_crash_log(&record);
        default(info);
    }));
}

fn append_crash_log(record: &str) {
    use std::os::unix::fs::OpenOptionsExt;
    let path = crate::paths::state_dir().join("crash.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > CRASH_LOG_MAX_BYTES) {
        crate::instance::rotate(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(&path) {
        let _ = f.write_all(record.as_bytes());
    }
}

/// "2026-09-25T08:53:33Z" (UTC) without a date/time dependency.
fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_utc(secs)
}

fn format_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's days-to-civil algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_contains_a_panic_and_reports_it() {
        let ok = guarded("test", || panic!("boom"));
        assert!(!ok);
        let mut ran = false;
        assert!(guarded("test", || ran = true));
        assert!(ran);
    }

    #[test]
    fn utc_formatting() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1_790_322_813), "2026-09-25T07:53:33Z");
        assert_eq!(format_utc(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn crash_record_has_time_location_and_message() {
        let r = crash_record("2026-09-25T08:53:33Z", "clipboard monitor", Some("src/x.rs:12:5"), "already borrowed");
        assert!(r.starts_with("2026-09-25T08:53:33Z"));
        assert!(r.contains("clipboard monitor"));
        assert!(r.contains("src/x.rs:12:5"));
        assert!(r.contains("already borrowed"));
        assert!(r.ends_with('\n'));
    }
}
