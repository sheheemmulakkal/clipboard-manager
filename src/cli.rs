//! Command-line interface: parsing and output formatting (pure).

use crate::clipboard::entry::ClipboardEntry;
use crate::ui::format;
use crate::ui::item_row::entry_kind;

pub const USAGE: &str = "\
Usage: clipboard-manager [COMMAND]

Without a command, starts the clipboard manager in the background
(or opens the popup if it is already running).

Commands (sent to the running instance):
  show            Open the clipboard history popup
  toggle          Open the popup, or close it if open
  pause           Stop recording the clipboard
  resume          Start recording again
  toggle-pause    Pause or resume
  clear           Remove all unpinned items
  list [--limit N]  Print the most recent items (default 20)
  quit            Stop the running instance
  reload          Restart the running instance (re-reads config.toml)

Options:
  -h, --help      Show this help
  -V, --version   Show the version
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Start,
    Show,
    Toggle,
    Pause,
    Resume,
    TogglePause,
    Clear,
    List { limit: usize },
    Quit,
    Reload,
    Help,
    Version,
}

impl Command {
    /// Commands that are executed by the running instance.
    pub fn is_remote(&self) -> bool {
        !matches!(
            self,
            Command::Start | Command::Help | Command::Version | Command::Reload | Command::List { .. }
        )
    }
}

/// Parse `argv` (including the program name).
pub fn parse(args: &[String]) -> Result<Command, String> {
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let no_more = |cmd: Command, extra: &[&str]| {
        if extra.is_empty() { Ok(cmd) } else { Err(format!("unexpected argument '{}'", extra[0])) }
    };
    match rest.as_slice() {
        [] => Ok(Command::Start),
        ["-h" | "--help" | "help", ..] => Ok(Command::Help),
        ["-V" | "--version", ..] => Ok(Command::Version),
        ["show", x @ ..] => no_more(Command::Show, x),
        ["toggle", x @ ..] => no_more(Command::Toggle, x),
        ["pause", x @ ..] => no_more(Command::Pause, x),
        ["resume", x @ ..] => no_more(Command::Resume, x),
        ["toggle-pause", x @ ..] => no_more(Command::TogglePause, x),
        ["clear", x @ ..] => no_more(Command::Clear, x),
        ["quit", x @ ..] => no_more(Command::Quit, x),
        ["reload", x @ ..] => no_more(Command::Reload, x),
        ["list"] => Ok(Command::List { limit: 20 }),
        ["list", "--limit" | "-n", n] => n
            .parse()
            .map(|limit| Command::List { limit })
            .map_err(|_| format!("invalid limit '{n}'")),
        [other, ..] => Err(format!("unknown command '{other}' (see --help)")),
    }
}

const TOKEN_ARG: &str = "--activation-token=";

/// Insert the launcher's startup-notification / XDG activation token (from
/// the environment of the forwarding process) as a hidden first argument.
/// The running instance passes it to GTK so the window manager lets the
/// popup take focus.
pub fn with_activation_token(args: &[String], token: Option<&str>) -> Vec<String> {
    let mut out = args.to_vec();
    if let Some(t) = token.filter(|t| !t.is_empty()) {
        out.insert(1.min(out.len()), format!("{TOKEN_ARG}{t}"));
    }
    out
}

/// Remove the hidden token argument; returns the remaining args and the token.
pub fn split_activation_token(args: &[String]) -> (Vec<String>, Option<String>) {
    let mut token = None;
    let rest = args
        .iter()
        .filter(|a| match a.strip_prefix(TOKEN_ARG) {
            Some(t) => {
                token = Some(t.to_string());
                false
            }
            None => true,
        })
        .cloned()
        .collect();
    (rest, token)
}

/// `list` output: one line per entry, newest first (`entries` already sorted).
pub fn format_list(entries: &[ClipboardEntry], limit: usize) -> String {
    if entries.is_empty() {
        return "(history is empty)\n".into();
    }
    let mut out = String::new();
    for (i, e) in entries.iter().take(limit).enumerate() {
        let kind = entry_kind(e, &[]);
        let mut preview = format::subtitle(e);
        if preview.chars().count() > 70 {
            preview = preview.chars().take(69).collect::<String>() + "\u{2026}";
        }
        out.push_str(&format!(
            "{:>2} {} {:<10} {}\n",
            i + 1,
            if e.pinned { '*' } else { ' ' },
            format::title(e, kind),
            preview
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::entry::ClipboardEntry;

    fn args(a: &[&str]) -> Vec<String> {
        std::iter::once("clipboard-manager").chain(a.iter().copied()).map(String::from).collect()
    }

    #[test]
    fn parses_commands() {
        assert_eq!(parse(&args(&[])), Ok(Command::Start));
        assert_eq!(parse(&args(&["show"])), Ok(Command::Show));
        assert_eq!(parse(&args(&["toggle"])), Ok(Command::Toggle));
        assert_eq!(parse(&args(&["pause"])), Ok(Command::Pause));
        assert_eq!(parse(&args(&["resume"])), Ok(Command::Resume));
        assert_eq!(parse(&args(&["toggle-pause"])), Ok(Command::TogglePause));
        assert_eq!(parse(&args(&["clear"])), Ok(Command::Clear));
        assert_eq!(parse(&args(&["list"])), Ok(Command::List { limit: 20 }));
        assert_eq!(parse(&args(&["list", "--limit", "5"])), Ok(Command::List { limit: 5 }));
        assert_eq!(parse(&args(&["quit"])), Ok(Command::Quit));
        assert_eq!(parse(&args(&["reload"])), Ok(Command::Reload));
        assert_eq!(parse(&args(&["--help"])), Ok(Command::Help));
        assert_eq!(parse(&args(&["-V"])), Ok(Command::Version));
    }

    #[test]
    fn activation_token_is_split_off() {
        let (rest, token) = split_activation_token(&args(&["--activation-token=abc_123", "toggle"]));
        assert_eq!(rest, args(&["toggle"]));
        assert_eq!(token.as_deref(), Some("abc_123"));
        let (rest, token) = split_activation_token(&args(&["show"]));
        assert_eq!(rest, args(&["show"]));
        assert_eq!(token, None);
        assert_eq!(with_activation_token(&args(&["toggle"]), Some("t1")), args(&["--activation-token=t1", "toggle"]));
        assert_eq!(with_activation_token(&args(&["toggle"]), None), args(&["toggle"]));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse(&args(&["frobnicate"])).is_err());
        assert!(parse(&args(&["list", "--limit", "x"])).is_err());
        assert!(parse(&args(&["show", "extra"])).is_err());
    }

    #[test]
    fn list_output() {
        let mut a = ClipboardEntry::new_text(1, "hello\nworld".into());
        a.pinned = true;
        let b = ClipboardEntry::new_text(2, "https://example.com".into());
        let out = format_list(&[a, b], 10);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with(" 1 *"));
        assert!(lines[0].contains("hello world"));
        assert!(lines[1].contains("URL"));
        assert_eq!(format_list(&[], 10), "(history is empty)\n");
    }
}
