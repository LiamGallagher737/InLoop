//! Human output: colour, aligned tables, Markdown in the terminal, a pager
//! (docs/06, D-C21Z). JSON output bypasses all of this.

use std::io::{IsTerminal, Write};
use std::process::{Command, Stdio};

#[derive(Clone, Copy)]
pub struct Style {
    pub color: bool,
    pub width: usize,
    pub tty: bool,
}

impl Style {
    pub fn detect() -> Style {
        let tty = std::io::stdout().is_terminal();
        let color = tty && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
        let width = terminal_size::terminal_size()
            .map(|(w, _)| w.0 as usize)
            .or_else(|| std::env::var("COLUMNS").ok().and_then(|c| c.parse().ok()))
            .unwrap_or(100)
            .clamp(40, 160);
        Style { color, width, tty }
    }

    #[cfg(test)]
    pub fn plain(width: usize) -> Style {
        Style {
            color: false,
            width,
            tty: false,
        }
    }

    fn wrap(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    pub fn bold(&self, s: &str) -> String {
        self.wrap("1", s)
    }
    pub fn dim(&self, s: &str) -> String {
        self.wrap("2", s)
    }
    pub fn green(&self, s: &str) -> String {
        self.wrap("32", s)
    }
    pub fn yellow(&self, s: &str) -> String {
        self.wrap("33", s)
    }
    pub fn red(&self, s: &str) -> String {
        self.wrap("31", s)
    }
    pub fn cyan(&self, s: &str) -> String {
        self.wrap("36", s)
    }
    pub fn magenta(&self, s: &str) -> String {
        self.wrap("35", s)
    }

    /// Colour a decision or task status.
    pub fn status(&self, s: &str) -> String {
        match s {
            "accepted" | "merged" => self.green(s),
            "proposed" | "review" | "ready" => self.yellow(s),
            "rejected" => self.red(s),
            "superseded" => self.dim(s),
            "running" => self.cyan(s),
            x if x.starts_with("blocked") => self.magenta(s),
            _ => s.to_string(),
        }
    }
}

/// Display width of a string, ignoring ANSI escapes.
pub fn visible_len(s: &str) -> usize {
    let mut n = 0;
    let mut in_esc = false;
    for c in s.chars() {
        if in_esc {
            if c == 'm' {
                in_esc = false;
            }
        } else if c == '\x1b' {
            in_esc = true;
        } else {
            n += 1;
        }
    }
    n
}

/// Cut `s` to `max` visible characters with an ellipsis. Plain text only.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Pad to `width` visible characters.
pub fn pad(s: &str, width: usize) -> String {
    let n = visible_len(s);
    if n >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - n))
    }
}

/// Wrap plain text to `width`, keeping words whole.
pub fn wrap_text(text: &str, width: usize, indent: &str) -> String {
    let width = width.max(20);
    let mut out = String::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && visible_len(&line) + 1 + visible_len(word) + indent.len() > width {
            out.push_str(indent);
            out.push_str(&line);
            out.push('\n');
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push_str(indent);
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn inline(style: &Style, s: &str) -> String {
    // **bold** and `code`.
    let mut out = String::new();
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("**")
            && let Some(end) = r.find("**")
        {
            out.push_str(&style.bold(&r[..end]));
            rest = &r[end + 2..];
            continue;
        }
        if let Some(r) = rest.strip_prefix('`')
            && let Some(end) = r.find('`')
        {
            out.push_str(&style.cyan(&r[..end]));
            rest = &r[end + 1..];
            continue;
        }
        let mut chars = rest.chars();
        if let Some(c) = chars.next() {
            out.push(c);
        }
        rest = chars.as_str();
    }
    out
}

/// Render Markdown for the terminal: headings, paragraphs wrapped to the
/// width, lists, code blocks left alone.
pub fn markdown(style: &Style, md: &str) -> String {
    let mut out = String::new();
    let mut para: Vec<String> = Vec::new();
    let mut in_code = false;
    let flush = |para: &mut Vec<String>, out: &mut String| {
        if !para.is_empty() {
            let text = para.join(" ");
            out.push_str(&wrap_text(&inline(style, &text), style.width, ""));
            para.clear();
        }
    };
    for line in md.lines() {
        if line.trim_start().starts_with("```") {
            flush(&mut para, &mut out);
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.push_str("    ");
            out.push_str(&style.dim(line));
            out.push('\n');
            continue;
        }
        let t = line.trim_end();
        if t.is_empty() {
            flush(&mut para, &mut out);
            if !out.ends_with("\n\n") && !out.is_empty() {
                out.push('\n');
            }
        } else if let Some(h) = t.strip_prefix("#") {
            flush(&mut para, &mut out);
            let h = h.trim_start_matches('#').trim();
            out.push_str(&style.bold(&style.cyan(h)));
            out.push('\n');
        } else if let Some(item) = t
            .strip_prefix("- ")
            .or_else(|| t.strip_prefix("* "))
            .or_else(|| t.trim_start().strip_prefix("- "))
        {
            flush(&mut para, &mut out);
            let wrapped = wrap_text(&inline(style, item), style.width.saturating_sub(2), "  ");
            out.push('•');
            out.push_str(wrapped.strip_prefix(" ").unwrap_or(&wrapped));
        } else if t.starts_with('|') || t.starts_with("    ") {
            flush(&mut para, &mut out);
            out.push_str(t);
            out.push('\n');
        } else {
            para.push(t.trim().to_string());
        }
    }
    flush(&mut para, &mut out);
    out.trim_end().to_string() + "\n"
}

/// A table with aligned columns; the last column takes what's left and is
/// truncated to fit.
pub fn table(style: &Style, header: &[&str], rows: &[Vec<String>]) -> String {
    let cols = header.len();
    let mut widths: Vec<usize> = header.iter().map(|h| h.len()).collect();
    for r in rows {
        for (i, c) in r.iter().enumerate().take(cols) {
            widths[i] = widths[i].max(visible_len(c));
        }
    }
    let mut out = String::new();
    let head: Vec<String> = header
        .iter()
        .enumerate()
        .map(|(i, h)| pad(h, widths[i]))
        .collect();
    out.push_str(&style.dim(head.join("  ").trim_end()));
    out.push('\n');
    let fixed: usize = widths
        .iter()
        .take(cols.saturating_sub(1))
        .map(|w| w + 2)
        .sum();
    let last_max = style.width.saturating_sub(fixed).max(10);
    for r in rows {
        let mut line = String::new();
        for (i, c) in r.iter().enumerate().take(cols) {
            if i + 1 == cols {
                if visible_len(c) > last_max && visible_len(c) == c.chars().count() {
                    line.push_str(&truncate(c, last_max));
                } else {
                    line.push_str(c);
                }
            } else {
                line.push_str(&pad(c, widths[i]));
                line.push_str("  ");
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// Print `text`, through a pager if it's long and stdout is a terminal.
pub fn page(style: &Style, text: &str) {
    let height = terminal_size::terminal_size()
        .map(|(_, h)| h.0 as usize)
        .unwrap_or(40);
    if style.tty
        && text.lines().count() + 1 > height
        && std::env::var_os("INLOOP_NO_PAGER").is_none()
    {
        let pager = std::env::var("PAGER").unwrap_or_else(|_| "less -FRX".into());
        let mut parts = pager.split_whitespace();
        if let Some(cmd) = parts.next()
            && let Ok(mut child) = Command::new(cmd)
                .args(parts)
                .env(
                    "LESS",
                    std::env::var("LESS").unwrap_or_else(|_| "FRX".into()),
                )
                .stdin(Stdio::piped())
                .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return;
        }
    }
    print!("{text}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_wraps_and_formats() {
        let s = Style::plain(30);
        let out = markdown(
            &s,
            "## Context\n\nThis is a fairly long paragraph that should wrap nicely.\n\n- one item\n- `two`\n",
        );
        insta::assert_snapshot!(out, @r"
        Context

        This is a fairly long
        paragraph that should wrap
        nicely.

        • one item
        • two
        ");
    }

    #[test]
    fn tables_align() {
        let s = Style::plain(40);
        let out = table(
            &s,
            &["ID", "TITLE"],
            &[
                vec!["D-AAAA".into(), "Short".into()],
                vec![
                    "D-B".into(),
                    "A very long title that will not fit in forty columns".into(),
                ],
            ],
        );
        insta::assert_snapshot!(out, @r"
        ID      TITLE
        D-AAAA  Short
        D-B     A very long title that will not…
        ");
    }
}
