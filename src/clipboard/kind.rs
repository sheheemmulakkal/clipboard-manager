//! Classify clipboard text so the UI can show a fitting icon and title.
//!
//! Heuristics only — a wrong guess just means a less fitting icon.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContentKind {
    Text,
    Url,
    Email,
    Path,
    Shell,
    Code,
    Color,
    Secret,
    Image,
    Screenshot,
}

/// First words that make a single line look like a shell command.
const SHELL_COMMANDS: &[&str] = &[
    "apt", "cargo", "cat", "cd", "chmod", "chown", "cp", "curl", "docker", "echo", "export",
    "find", "git", "grep", "journalctl", "kubectl", "ln", "ls", "make", "mkdir", "mv", "npm",
    "npx", "pip", "pnpm", "python", "python3", "rm", "rsync", "scp", "sed", "ssh", "systemctl",
    "tar", "wget", "yarn",
];

/// Substrings typical of source code; two or more ⇒ code.
const CODE_TOKENS: &[&str] = &[
    "{", "}", ";", "=>", "->", "::", "()", "fn ", "def ", "class ", "import ", "use ",
    "let ", "const ", "var ", "return ", "#include", "</", "/>", "==",
];

/// Prefixes of well-known API tokens / key material.
const SECRET_PREFIXES: &[&str] = &[
    "ghp_", "gho_", "ghs_", "github_pat_", "glpat-", "sk-", "xoxb-", "xoxp-", "AKIA", "-----BEGIN",
];

impl ContentKind {
    pub fn detect(text: &str) -> ContentKind {
        let t = text.trim();
        if t.is_empty() {
            return ContentKind::Text;
        }
        let single_line = !t.contains('\n');
        let single_token = single_line && !t.contains(char::is_whitespace);

        if single_token && is_url(t) {
            ContentKind::Url
        } else if single_token && is_email(t) {
            ContentKind::Email
        } else if single_line && is_color(t) {
            ContentKind::Color
        } else if single_token && is_path(t) {
            ContentKind::Path
        } else if is_secret(t, single_token) {
            ContentKind::Secret
        } else if single_line && is_shell(t) {
            ContentKind::Shell
        } else if CODE_TOKENS.iter().filter(|tok| t.contains(**tok)).count() >= 2 {
            ContentKind::Code
        } else {
            ContentKind::Text
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            ContentKind::Text => "Text",
            ContentKind::Url => "URL",
            ContentKind::Email => "Email",
            ContentKind::Path => "Path",
            ContentKind::Shell => "Shell",
            ContentKind::Code => "Code",
            ContentKind::Color => "Colour",
            ContentKind::Secret => "Secret",
            ContentKind::Image => "Image",
            ContentKind::Screenshot => "Screenshot",
        }
    }
}

fn is_url(t: &str) -> bool {
    let lower = t.to_ascii_lowercase();
    ["http://", "https://", "ftp://", "file://"]
        .iter()
        .any(|p| lower.starts_with(p) && lower.len() > p.len())
        || (lower.starts_with("www.") && lower[4..].contains('.'))
}

fn is_email(t: &str) -> bool {
    let Some((local, domain)) = t.split_once('@') else { return false };
    if local.is_empty() || domain.contains('@') {
        return false;
    }
    let Some((host, tld)) = domain.rsplit_once('.') else { return false };
    !host.is_empty() && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
}

fn is_color(t: &str) -> bool {
    if let Some(hex) = t.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    let lower = t.to_ascii_lowercase();
    ["rgb(", "rgba(", "hsl(", "hsla("].iter().any(|p| lower.starts_with(p)) && lower.ends_with(')')
}

fn is_path(t: &str) -> bool {
    ["/", "~/", "./", "../"].iter().any(|p| t.starts_with(p)) && t.len() > 1
}

fn is_shell(t: &str) -> bool {
    if t.starts_with("$ ") || t.starts_with("sudo ") {
        return true;
    }
    match t.split_once(' ') {
        Some((first, _)) => SHELL_COMMANDS.contains(&first),
        None => false,
    }
}

fn is_secret(t: &str, single_token: bool) -> bool {
    if SECRET_PREFIXES.iter().any(|p| t.starts_with(p)) {
        return true;
    }
    if !single_token {
        return false;
    }
    if is_uuid(t) || (t.len() >= 32 && t.chars().all(|c| c.is_ascii_hexdigit())) {
        return true;
    }
    if !(16..=128).contains(&t.len()) {
        return false;
    }
    // Mixed-case letters plus digits in one long token: password / API key.
    t.chars().any(|c| c.is_ascii_lowercase())
        && t.chars().any(|c| c.is_ascii_uppercase())
        && t.chars().any(|c| c.is_ascii_digit())
}

fn is_uuid(t: &str) -> bool {
    let parts: Vec<&str> = t.split('-').collect();
    parts.len() == 5
        && parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12])
        && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_kinds() {
        use ContentKind::*;
        for (s, k) in [
            ("https://github.com/x/y", Url),
            ("www.example.com", Url),
            ("  https://example.com/a?b=c  ", Url),
            ("me@example.com", Email),
            ("#f97316", Color),
            ("#fff", Color),
            ("rgb(1, 2, 3)", Color),
            ("/usr/bin/env", Path),
            ("~/notes.txt", Path),
            ("./target/debug/app", Path),
            ("git status", Shell),
            ("$ cargo build --release", Shell),
            ("sudo apt install foo", Shell),
            ("find . -name \"Cargo.toml\" -not-path \"*/t\"", Shell),
            ("use std::collections::HashSet;", Code),
            ("fn main() {\n    println!(\"hi\");\n}", Code),
            ("const x = () => { return 1; };", Code),
            ("ghp_abcdefghijklmnopqrstuvwxyz0123", Secret),
            ("f9c0e003-3d16-4858-a405-e41ca0d1c2b3", Secret),
            ("sk-proj-AbC123xYz789QwErTy456", Secret),
            ("Xk9#mP2$vL7qR4!n", Secret),
            ("analyze this project. I need improvements", Text),
            ("hello", Text),
            ("Shairon@p123", Text),
            ("Meeting at 5pm; bring notes", Text),
            ("", Text),
        ] {
            assert_eq!(ContentKind::detect(s), k, "{s:?}");
        }
    }

    #[test]
    fn titles_are_human_readable() {
        assert_eq!(ContentKind::Url.title(), "URL");
        assert_eq!(ContentKind::Text.title(), "Text");
        assert_eq!(ContentKind::Screenshot.title(), "Screenshot");
    }
}
