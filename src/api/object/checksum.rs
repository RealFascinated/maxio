use axum::body::Body;
use axum::http::HeaderMap;
use futures::TryStreamExt;
use tokio::io::{AsyncBufReadExt, AsyncReadExt};

use crate::error::S3Error;
use crate::storage::ChecksumAlgorithm;

/// Extract checksum algorithm and optional expected value from request headers.
pub(crate) fn extract_checksum(headers: &HeaderMap) -> Option<(ChecksumAlgorithm, Option<String>)> {
    let pairs = [
        ("x-amz-checksum-crc32", ChecksumAlgorithm::CRC32),
        ("x-amz-checksum-crc32c", ChecksumAlgorithm::CRC32C),
        ("x-amz-checksum-sha1", ChecksumAlgorithm::SHA1),
        ("x-amz-checksum-sha256", ChecksumAlgorithm::SHA256),
    ];

    // Check for a value header first (implies the algorithm)
    for (header, algo) in &pairs {
        if let Some(val) = headers.get(*header).and_then(|v| v.to_str().ok()) {
            return Some((*algo, Some(val.to_string())));
        }
    }

    // Fall back to algorithm-only header (compute but don't validate)
    headers
        .get("x-amz-checksum-algorithm")
        .and_then(|v| v.to_str().ok())
        .and_then(ChecksumAlgorithm::from_header_str)
        .map(|algo| (algo, None))
}

pub(super) fn add_checksum_header(
    builder: http::response::Builder,
    meta: &crate::storage::ObjectMeta,
) -> http::response::Builder {
    if let (Some(algo), Some(val)) = (&meta.checksum_algorithm, &meta.checksum_value) {
        builder.header(algo.header_name(), val.as_str())
    } else {
        builder
    }
}

async fn read_aws_chunk<R: tokio::io::AsyncRead + Unpin>(
    mut reader: tokio::io::BufReader<R>,
) -> std::io::Result<(Option<Vec<u8>>, tokio::io::BufReader<R>)> {
    let mut line = String::new();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        return Ok((None, reader));
    }
    let line = line.trim_end_matches(|c| c == '\r' || c == '\n');
    let size_str = line.split(';').next().unwrap_or("0");
    let chunk_size = usize::from_str_radix(size_str.trim(), 16)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid chunk size"))?;
    if chunk_size == 0 {
        // AWS spec: `0;chunk-signature=…\r\n\r\n`. Some clients omit the final CRLF;
        // accept EOF here (matches the pre-streaming decoder).
        let mut crlf = [0u8; 2];
        if reader.read_exact(&mut crlf).await.is_ok() && crlf != [b'\r', b'\n'] {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid chunk terminator",
            ));
        }
        return Ok((None, reader));
    }
    let mut chunk = vec![0u8; chunk_size];
    reader.read_exact(&mut chunk).await?;
    let mut crlf = [0u8; 2];
    reader.read_exact(&mut crlf).await?;
    Ok((Some(chunk), reader))
}

pub(crate) async fn body_to_reader(
    headers: &HeaderMap,
    body: Body,
) -> Result<std::pin::Pin<Box<dyn tokio::io::AsyncRead + Send>>, S3Error> {
    let is_aws_chunked = headers
        .get("x-amz-content-sha256")
        .and_then(|v| v.to_str().ok())
        == Some("STREAMING-AWS4-HMAC-SHA256-PAYLOAD");

    let stream = body.into_data_stream();
    let raw_reader = tokio_util::io::StreamReader::new(
        stream.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)),
    );

    if is_aws_chunked {
        let chunked = futures::stream::try_unfold(
            tokio::io::BufReader::new(raw_reader),
            |reader| async move {
                let (chunk, reader) = read_aws_chunk(reader).await?;
                let item: std::io::Result<Option<(bytes::Bytes, _)>> =
                    Ok(chunk.map(|c| (bytes::Bytes::from(c), reader)));
                item
            },
        );
        Ok(Box::pin(tokio_util::io::StreamReader::new(chunked)))
    } else {
        Ok(Box::pin(raw_reader))
    }
}
