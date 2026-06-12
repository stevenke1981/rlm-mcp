use crate::error::{Error, Result};
use std::io::{self, BufRead, Write};

/// Read one JSON-RPC message (Content-Length framed or newline-delimited).
pub fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<String>> {
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        headers.push_str(trimmed);
        headers.push('\n');
    }

    if let Some(len) = parse_content_length(&headers) {
        let mut body = vec![0u8; len];
        reader.read_exact(&mut body)?;
        return Ok(Some(
            String::from_utf8(body).map_err(|e| Error::Other(e.to_string()))?,
        ));
    }

    if let Some(first_line) = headers.lines().next() {
        if first_line.starts_with('{') {
            return Ok(Some(first_line.to_string()));
        }
    }
    Ok(None)
}

fn parse_content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        let lower = line.to_lowercase();
        if lower.starts_with("content-length:") {
            return lower.split(':').nth(1)?.trim().parse().ok();
        }
    }
    None
}

/// Write JSON-RPC message with Content-Length framing.
pub fn write_message<W: Write>(writer: &mut W, body: &str) -> Result<()> {
    writeln!(writer, "Content-Length: {}", body.len())?;
    writeln!(writer)?;
    writer.write_all(body.as_bytes())?;
    writer.flush()?;
    Ok(())
}

pub fn read_stdin_message() -> Result<Option<String>> {
    let stdin = io::stdin();
    let mut lock = stdin.lock();
    read_message(&mut lock)
}

pub fn write_stdout_message(body: &str) -> Result<()> {
    let mut stdout = io::stdout();
    write_message(&mut stdout, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parses_content_length_frame() {
        let data = "Content-Length: 13\r\n\r\n{\"json\":true}";
        let mut cursor = Cursor::new(data.as_bytes());
        let msg = read_message(&mut cursor).unwrap().unwrap();
        assert_eq!(msg, "{\"json\":true}");
    }
}
