/// Produce syntax-highlighted HTML from YAML text.
///
/// Returns a string of `<span class="hl-*">…</span>` fragments safe for
/// use inside `<pre><code>`.  All text is HTML-escaped first.
pub fn highlight_yaml(src: &str) -> String {
    let mut out = String::with_capacity(src.len() * 2);

    for line in src.split('\n') {
        highlight_line(line, &mut out);
        out.push('\n');
    }

    // Remove trailing newline added by the loop
    if out.ends_with('\n') {
        out.pop();
    }

    out
}

fn highlight_line(line: &str, out: &mut String) {
    let trimmed = line.trim();

    // Empty line
    if trimmed.is_empty() {
        return;
    }

    // Full-line comment
    if trimmed.starts_with('#') {
        push_span(out, "comment", &html_escape(line));
        return;
    }

    // Document separator
    if trimmed == "---" || trimmed == "..." {
        push_span(out, "separator", &html_escape(line));
        return;
    }

    // Leading whitespace (indentation)
    let indent_len = line.len() - line.trim_start().len();
    if indent_len > 0 {
        out.push_str(&html_escape(&line[..indent_len]));
    }

    let rest = &line[indent_len..];

    // List item prefix "- "
    let content = if let Some(after_dash) = rest.strip_prefix("- ") {
        push_span(out, "punctuation", "- ");
        after_dash
    } else {
        rest
    };

    // Key: value
    if let Some(colon_pos) = find_yaml_colon(content) {
        let key = &content[..colon_pos];
        let after_colon = &content[colon_pos + 1..];

        push_span(out, "key", &html_escape(key));
        push_span(out, "punctuation", ":");

        if !after_colon.is_empty() {
            highlight_value(after_colon, out);
        }
    } else {
        // Plain scalar or list item value
        highlight_value(content, out);
    }
}

/// Find the position of the first `:` that acts as a YAML key separator
/// (followed by a space or end-of-string, and not inside quotes).
fn find_yaml_colon(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut in_single = false;
    let mut in_double = false;

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b':' if !in_single && !in_double => {
                // Must be followed by space or end of string
                if i + 1 >= bytes.len() || bytes[i + 1] == b' ' {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn highlight_value(raw: &str, out: &mut String) {
    let trimmed = raw.trim();

    // Preserve leading space
    let leading = raw.len() - raw.trim_start().len();
    if leading > 0 {
        out.push_str(&html_escape(&raw[..leading]));
    }

    if trimmed.is_empty() {
        return;
    }

    // Inline comment at end: split value and comment
    if let Some((val, comment)) = split_trailing_comment(trimmed) {
        classify_scalar(val.trim(), out);
        out.push(' ');
        push_span(out, "comment", &html_escape(comment));
        return;
    }

    classify_scalar(trimmed, out);
}

/// Classify a scalar value and push the appropriate span.
fn classify_scalar(val: &str, out: &mut String) {
    if val.is_empty() {
        return;
    }

    // Quoted strings
    if (val.starts_with('"') && val.ends_with('"'))
        || (val.starts_with('\'') && val.ends_with('\''))
    {
        push_span(out, "string", &html_escape(val));
        return;
    }

    // Boolean
    if matches!(
        val,
        "true" | "false" | "True" | "False" | "TRUE" | "FALSE" | "yes" | "no" | "Yes" | "No"
    ) {
        push_span(out, "boolean", &html_escape(val));
        return;
    }

    // Null
    if matches!(val, "null" | "Null" | "NULL" | "~") {
        push_span(out, "null", &html_escape(val));
        return;
    }

    // Number (integer or float)
    if val.parse::<f64>().is_ok() || val.starts_with("0x") || val.starts_with("0o") {
        push_span(out, "number", &html_escape(val));
        return;
    }

    // Anchor / alias
    if val.starts_with('*') || val.starts_with('&') {
        push_span(out, "anchor", &html_escape(val));
        return;
    }

    // Block scalar indicator
    if val == "|" || val == ">" || val == "|-" || val == ">-" || val == "|+" || val == ">+" {
        push_span(out, "punctuation", &html_escape(val));
        return;
    }

    // Default: plain string value
    push_span(out, "string", &html_escape(val));
}

/// Try to split a trailing `# comment` from a value.
fn split_trailing_comment(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    let mut in_single = false;
    let mut in_double = false;

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'#' if !in_single && !in_double && i > 0 && bytes[i - 1] == b' ' => {
                return Some((&s[..i - 1], &s[i..]));
            }
            _ => {}
        }
    }
    None
}

fn push_span(out: &mut String, class: &str, content: &str) {
    out.push_str("<span class=\"hl-");
    out.push_str(class);
    out.push_str("\">");
    out.push_str(content);
    out.push_str("</span>");
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
