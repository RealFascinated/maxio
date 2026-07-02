use axum::http::HeaderMap;

use super::algorithm::ChecksumAlgorithm;

#[derive(Debug, Default, Clone)]
struct ChecksumTrailers {
    values: Vec<(ChecksumAlgorithm, String)>,
}

impl ChecksumTrailers {
    pub fn from_trailer_lines(lines: &[String]) -> Self {
        let mut values = Vec::new();
        for line in lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some((name, val)) = line.split_once(':') else {
                continue;
            };
            let Some(algo) = ChecksumAlgorithm::from_request_header(name.trim()) else {
                continue;
            };
            values.push((algo, val.trim().to_string()));
        }
        Self { values }
    }
}

fn extract_checksum(
    headers: &HeaderMap,
    trailers: &ChecksumTrailers,
) -> Option<(ChecksumAlgorithm, Option<String>)> {
    for algo in ChecksumAlgorithm::all() {
        if let Some(val) = headers
            .get(algo.header_name())
            .and_then(|v| v.to_str().ok())
        {
            return Some((*algo, Some(val.to_string())));
        }
    }

    if let Some((algo, val)) = trailers.values.first() {
        return Some((*algo, Some(val.clone())));
    }

    headers
        .get("x-amz-checksum-algorithm")
        .and_then(|v| v.to_str().ok())
        .and_then(ChecksumAlgorithm::from_header_str)
        .map(|algo| (algo, None))
}

/// Checksum expectation from headers, or algorithm-only when sent in a streaming trailer.
pub fn extract_upload_checksum(headers: &HeaderMap) -> Option<(ChecksumAlgorithm, Option<String>)> {
    extract_checksum(headers, &ChecksumTrailers::default())
        .or_else(|| infer_trailer_checksum_algorithm(headers).map(|algo| (algo, None)))
}

fn infer_trailer_checksum_algorithm(headers: &HeaderMap) -> Option<ChecksumAlgorithm> {
    headers
        .get("x-amz-trailer")
        .and_then(|v| v.to_str().ok())
        .and_then(|names| {
            names.split(',').map(str::trim).find_map(|entry| {
                let name = entry.split(':').next().unwrap_or(entry).trim();
                ChecksumAlgorithm::from_request_header(name)
            })
        })
}

/// After a streaming upload body is consumed, compare the stored checksum to the trailer.
pub fn stored_checksum_matches_trailer(trailer_lines: &[String], computed: Option<&str>) -> bool {
    let trailers = ChecksumTrailers::from_trailer_lines(trailer_lines);
    match extract_checksum(&HeaderMap::new(), &trailers) {
        Some((_, Some(expected))) => computed == Some(expected.as_str()),
        Some((_, None)) | None => true,
    }
}

pub fn add_checksum_header_from_meta(
    builder: http::response::Builder,
    meta: &crate::storage::ObjectMeta,
) -> http::response::Builder {
    if let (Some(algo), Some(val)) = (&meta.checksum_algorithm, &meta.checksum_value) {
        builder.header(algo.header_name(), val.as_str())
    } else {
        builder
    }
}
