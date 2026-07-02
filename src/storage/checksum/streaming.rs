use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::HeaderMap;
use futures::TryStreamExt;
use tokio::io::{AsyncBufReadExt, AsyncReadExt};

use crate::error::S3Error;
use crate::storage::ByteStream;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestBodyEncoding {
    Raw,
    AwsChunked { trailers: bool },
}

impl RequestBodyEncoding {
    fn from_headers(headers: &HeaderMap) -> Self {
        match headers
            .get("x-amz-content-sha256")
            .and_then(|v| v.to_str().ok())
        {
            Some("STREAMING-AWS4-HMAC-SHA256-PAYLOAD") => Self::AwsChunked { trailers: false },
            Some("STREAMING-AWS4-HMAC-SHA256-PAYLOAD-TRAILER")
            | Some("STREAMING-UNSIGNED-PAYLOAD-TRAILER") => Self::AwsChunked { trailers: true },
            _ => Self::Raw,
        }
    }

    async fn decode(self, body: Body) -> Result<DecodedRequestBody, S3Error> {
        let stream = body.into_data_stream();
        let raw_reader = tokio_util::io::StreamReader::new(
            stream.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)),
        );

        match self {
            Self::Raw => Ok(DecodedRequestBody {
                reader: Box::pin(raw_reader),
                trailer_lines: None,
            }),
            Self::AwsChunked { trailers } => {
                let trailer_lines = Arc::new(Mutex::new(Vec::new()));
                let capture = Arc::clone(&trailer_lines);
                let chunked = futures::stream::try_unfold(
                    (tokio::io::BufReader::new(raw_reader), trailers),
                    move |(reader, has_trailers)| {
                        let capture = Arc::clone(&capture);
                        async move {
                            let (chunk, reader, lines) =
                                read_aws_chunk(reader, has_trailers).await?;
                            if !lines.is_empty() {
                                capture.lock().unwrap().extend(lines);
                            }
                            Ok::<_, std::io::Error>(
                                chunk.map(|c| (bytes::Bytes::from(c), (reader, has_trailers))),
                            )
                        }
                    },
                );
                Ok(DecodedRequestBody {
                    reader: Box::pin(tokio_util::io::StreamReader::new(chunked)),
                    trailer_lines: Some(trailer_lines),
                })
            }
        }
    }
}

/// Request body after wire-format decoding.
pub struct DecodedRequestBody {
    pub reader: ByteStream,
    /// Populated after the reader is fully consumed (AWS chunked trailer uploads).
    trailer_lines: Option<Arc<Mutex<Vec<String>>>>,
}

impl DecodedRequestBody {
    pub(crate) fn trailer_lines_handle(&self) -> Option<Arc<Mutex<Vec<String>>>> {
        self.trailer_lines.clone()
    }
}

pub async fn decode_request_body(
    headers: &HeaderMap,
    body: Body,
) -> Result<DecodedRequestBody, S3Error> {
    RequestBodyEncoding::from_headers(headers)
        .decode(body)
        .await
}

async fn read_trailer_lines<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut tokio::io::BufReader<R>,
) -> std::io::Result<Vec<String>> {
    let mut lines = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        if line.trim_end_matches(['\r', '\n']).is_empty() {
            break;
        }
        lines.push(line.trim_end_matches(['\r', '\n']).to_string());
    }
    Ok(lines)
}

async fn read_aws_chunk<R: tokio::io::AsyncRead + Unpin>(
    mut reader: tokio::io::BufReader<R>,
    has_trailers: bool,
) -> std::io::Result<(Option<Vec<u8>>, tokio::io::BufReader<R>, Vec<String>)> {
    let mut line = String::new();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        return Ok((None, reader, Vec::new()));
    }
    let line = line.trim_end_matches(|c| c == '\r' || c == '\n');
    let size_str = line.split(';').next().unwrap_or("0");
    let chunk_size = usize::from_str_radix(size_str.trim(), 16)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid chunk size"))?;
    if chunk_size == 0 {
        let trailer_lines = if has_trailers {
            read_trailer_lines(&mut reader).await?
        } else {
            let mut crlf = [0u8; 2];
            if reader.read_exact(&mut crlf).await.is_ok() && crlf != [b'\r', b'\n'] {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "invalid chunk terminator",
                ));
            }
            Vec::new()
        };
        return Ok((None, reader, trailer_lines));
    }
    let mut chunk = vec![0u8; chunk_size];
    reader.read_exact(&mut chunk).await?;
    let mut crlf = [0u8; 2];
    reader.read_exact(&mut crlf).await?;
    Ok((Some(chunk), reader, Vec::new()))
}
