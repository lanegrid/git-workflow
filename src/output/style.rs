//! Styled console output utilities

/// ANSI color codes
const RED: &str = "\x1b[0;31m";
const GREEN: &str = "\x1b[0;32m";
const YELLOW: &str = "\x1b[0;33m";
const BLUE: &str = "\x1b[0;34m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

/// Output helper for informational messages (ℹ)
pub fn info(msg: &str) {
    println!("{BLUE}ℹ{RESET} {msg}");
}

/// Output helper for success messages (✓)
pub fn success(msg: &str) {
    println!("{GREEN}✓{RESET} {msg}");
}

/// Output helper for warning messages (⚠)
pub fn warn(msg: &str) {
    println!("{YELLOW}⚠{RESET} {msg}");
}

/// Output helper for error messages (✗)
pub fn error(msg: &str) {
    eprintln!("{RED}✗{RESET} {msg}");
}

/// Output helper for action/suggestion messages (→)
pub fn action(msg: &str) {
    println!("{BOLD}→{RESET} {msg}");
}

/// Format text as bold
pub fn bold(text: &str) -> String {
    format!("{BOLD}{text}{RESET}")
}

/// Print a horizontal separator
pub fn separator() {
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}

/// Print the "Ready" header with branch name
pub fn ready(message: &str, branch: &str) {
    println!();
    separator();
    println!("{GREEN}{BOLD}{message}{RESET} on {BOLD}{branch}{RESET}");
}

/// Print hints for next actions
pub fn hints(lines: &[&str]) {
    println!();
    println!("{BOLD}Next:{RESET}");
    for line in lines {
        println!("  {line}");
    }
}

/// Print a `Try:` footer (to stderr) listing runnable next commands after an error.
pub fn error_footer(lines: &[&str]) {
    eprintln!();
    eprintln!("{BOLD}Try:{RESET}");
    for line in lines {
        eprintln!("  {line}");
    }
}
