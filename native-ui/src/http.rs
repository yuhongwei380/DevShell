/// Normalize an HTTP/1.1 response before the API-specific decoder reads it.
/// Go switches large configuration/backup responses to chunked transfer; the
/// framing bytes are transport metadata, never JSON or downloaded file bytes.
pub(super) fn decode_response(wire: &[u8]) -> Result<Vec<u8>, String> {
    let end = wire
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .ok_or("HTTP 响应头不完整")?;
    let head = std::str::from_utf8(&wire[..end]).map_err(|_| "HTTP 响应头编码无效")?;
    let body = &wire[end + 4..];
    let mut chunked = false;
    let mut length = None;
    for line in head.lines().skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("transfer-encoding") {
                chunked = value
                    .split(',')
                    .any(|s| s.trim().eq_ignore_ascii_case("chunked"));
            }
            if name.eq_ignore_ascii_case("content-length") {
                length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .map_err(|_| "HTTP 内容长度无效")?,
                );
            }
        }
    }
    let bytes = if chunked {
        let mut output = Vec::new();
        let mut offset = 0usize;
        loop {
            let line_end = body
                .get(offset..)
                .and_then(|rest| rest.windows(2).position(|part| part == b"\r\n"))
                .ok_or("HTTP 分块头不完整")?
                + offset;
            let line =
                std::str::from_utf8(&body[offset..line_end]).map_err(|_| "HTTP 分块长度无效")?;
            let count = usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16)
                .map_err(|_| "HTTP 分块长度无效")?;
            offset = line_end + 2;
            if count == 0 {
                let trailers = body.get(offset..).ok_or("HTTP 分块结束标记不完整")?;
                if !trailers.starts_with(b"\r\n")
                    && !trailers.windows(4).any(|part| part == b"\r\n\r\n")
                {
                    return Err("HTTP 分块结束标记不完整".into());
                }
                break;
            }
            let end = offset.checked_add(count).ok_or("HTTP 分块长度溢出")?;
            let data = body.get(offset..end).ok_or("HTTP 分块内容不完整")?;
            if body.get(end..end + 2) != Some(b"\r\n".as_slice()) {
                return Err("HTTP 分块结束标记无效".into());
            }
            output.extend_from_slice(data);
            offset = end + 2;
        }
        output
    } else if let Some(length) = length {
        body.get(..length).ok_or("HTTP 响应内容不完整")?.to_vec()
    } else {
        body.to_vec()
    };
    let mut response = Vec::with_capacity(end + bytes.len() + 64);
    for line in head.lines() {
        if !line.to_ascii_lowercase().starts_with("transfer-encoding:")
            && !line.to_ascii_lowercase().starts_with("content-length:")
        {
            response.extend_from_slice(line.as_bytes());
            response.extend_from_slice(b"\r\n");
        }
    }
    response.extend_from_slice(format!("Content-Length: {}\r\n\r\n", bytes.len()).as_bytes());
    response.extend_from_slice(&bytes);
    Ok(response)
}

/// Copy a download with bounded buffering. A truncated response is an error;
/// callers write to a temporary file and commit it only after this succeeds.
pub(super) fn stream_download(
    reader: &mut impl std::io::BufRead,
    target: &mut impl std::io::Write,
) -> Result<u64, String> {
    use std::io::{BufRead, Read};
    fn line(reader: &mut impl std::io::BufRead, remaining: &mut usize) -> Result<String, String> {
        let mut bytes = Vec::new();
        let count = reader
            .take((*remaining + 1) as u64)
            .read_until(b'\n', &mut bytes)
            .map_err(|e| e.to_string())?;
        if count == 0 || count > *remaining || !bytes.ends_with(b"\r\n") {
            return Err("HTTP 响应头不完整或过长".into());
        }
        *remaining -= count;
        String::from_utf8(bytes[..count - 2].to_vec()).map_err(|_| "HTTP 响应头编码无效".into())
    }
    let mut limit = 65536;
    let status = line(reader, &mut limit)?;
    let status = status.split_whitespace().nth(1).unwrap_or("");
    let mut length = None;
    let mut chunked = false;
    loop {
        let header = line(reader, &mut limit)?;
        if header.is_empty() {
            break;
        }
        if let Some((key, value)) = header.split_once(':') {
            if key.eq_ignore_ascii_case("content-length") {
                length = Some(
                    value
                        .trim()
                        .parse::<u64>()
                        .map_err(|_| "HTTP 内容长度无效")?,
                );
            } else if key.eq_ignore_ascii_case("transfer-encoding") {
                chunked = value
                    .split(',')
                    .any(|v| v.trim().eq_ignore_ascii_case("chunked"));
            }
        }
    }
    if status != "200" {
        return Err(format!("下载失败：HTTP {status}"));
    }
    let mut total = 0u64;
    if chunked {
        loop {
            let mut chunk_limit = 8192;
            let header = line(reader, &mut chunk_limit)?;
            let count = u64::from_str_radix(header.split(';').next().unwrap_or("").trim(), 16)
                .map_err(|_| "HTTP 分块长度无效")?;
            if count == 0 {
                let mut trailer_limit = 65536;
                while !line(reader, &mut trailer_limit)?.is_empty() {}
                break;
            }
            let written =
                std::io::copy(&mut reader.take(count), target).map_err(|e| e.to_string())?;
            if written != count {
                return Err("下载内容不完整".into());
            }
            let mut end = [0; 2];
            reader
                .read_exact(&mut end)
                .map_err(|_| "HTTP 分块结束标记不完整")?;
            if end != *b"\r\n" {
                return Err("HTTP 分块结束标记无效".into());
            }
            total = total.checked_add(written).ok_or("下载长度溢出")?;
        }
    } else if let Some(length) = length {
        total = std::io::copy(&mut reader.take(length), target).map_err(|e| e.to_string())?;
        if total != length {
            return Err("下载内容不完整".into());
        }
    } else {
        total = std::io::copy(reader, target).map_err(|e| e.to_string())?;
    }
    Ok(total)
}

pub(super) fn download_to_file(
    reader: &mut impl std::io::BufRead,
    path: &std::path::Path,
) -> Result<u64, String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    let count = stream_download(reader, temporary.as_file_mut())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary.persist(path).map_err(|e| e.to_string())?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunked_large_config_and_binary_download_are_decoded_without_corruption() {
        let body=serde_json::json!({"servers":(0..100).map(|i|serde_json::json!({"id":i,"name":"中文连接"})).collect::<Vec<_>>()}).to_string().into_bytes();
        let mut wire = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        for chunk in body.chunks(7) {
            wire.extend_from_slice(format!("{:x};test=1\r\n", chunk.len()).as_bytes());
            wire.extend_from_slice(chunk);
            wire.extend_from_slice(b"\r\n");
        }
        wire.extend_from_slice(b"0\r\nX-Trailer: yes\r\n\r\n");
        let decoded = decode_response(&wire).unwrap();
        let start = decoded.windows(4).position(|s| s == b"\r\n\r\n").unwrap() + 4;
        assert_eq!(&decoded[start..], body);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&decoded[start..]).unwrap()["servers"]
                .as_array()
                .unwrap()
                .len(),
            100
        );
        let decoded = decode_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: Chunked\r\n\r\n3\r\n\x00\xff\x80\r\n0\r\n\r\n",
        )
        .unwrap();
        assert!(decoded.ends_with(b"\x00\xff\x80"));
    }
    #[test]
    fn truncated_framing_and_false_content_lengths_are_errors() {
        assert!(decode_response(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nx").is_err());
        assert!(
            decode_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nx")
                .is_err()
        );
    }
    #[test]
    fn failed_download_does_not_replace_an_existing_destination() {
        let dir =
            tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
                .unwrap();
        let path = dir.path().join("saved.bin");
        std::fs::write(&path, b"original").unwrap();
        assert!(
            download_to_file(
                &mut std::io::Cursor::new(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nx"),
                &path
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        download_to_file(
            &mut std::io::Cursor::new(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nnew"),
            &path,
        )
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }
    #[test]
    fn streaming_download_preserves_binary_and_rejects_truncation() {
        let wire=b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3;ext=yes\r\n\x00\xff\x80\r\n2\r\nAB\r\n0\r\nTrailer: yes\r\n\r\n";
        let mut result = Vec::new();
        assert_eq!(
            stream_download(&mut std::io::Cursor::new(wire), &mut result).unwrap(),
            5
        );
        assert_eq!(result, b"\x00\xff\x80AB");
        for wire in [
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nx".as_slice(),
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nab",
        ] {
            assert!(stream_download(&mut std::io::Cursor::new(wire), &mut Vec::new()).is_err());
        }
        let mut untouched = Vec::new();
        assert!(
            stream_download(
                &mut std::io::Cursor::new(
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 3\r\n\r\nbad"
                ),
                &mut untouched
            )
            .is_err()
        );
        assert!(untouched.is_empty());
    }
}
