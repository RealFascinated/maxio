//! Per-bucket LZ4 block compression for object payloads.
//!
//! Stored layout: `[block 0][block 1]...[length table][block count: u32][MAGIC]`.
//! Every block holds `BLOCK_SIZE` plaintext bytes (the last one may be shorter) and is
//! compressed independently, so range reads only decode the blocks they touch. The table
//! has one little-endian `u32` per block: the stored length, with `RAW_FLAG` set when the
//! block did not shrink and is stored verbatim.

use super::{ByteStream, StorageError};
use bytes::Bytes;
use futures::stream;
use lz4_flex::block::{compress_into, decompress_into, get_maximum_output_size};
use std::io::SeekFrom;
use std::path::Path;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt};
use tokio_util::io::StreamReader;

pub const BLOCK_SIZE: usize = 256 * 1024;
const RAW_FLAG: u32 = 1 << 31;
const MAGIC: [u8; 4] = *b"MXL4";
const FOOTER_SIZE: u64 = 8;
/// Objects smaller than this are stored raw: the table overhead outweighs any saving.
const MIN_COMPRESS_SIZE: usize = 1024;
/// Leading blocks that must all be incompressible before the encoder stops compressing.
const INCOMPRESSIBLE_PROBE_BLOCKS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression {
    #[default]
    None,
    Lz4,
}

impl Compression {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Lz4 => "lz4",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "lz4" => Some(Self::Lz4),
            _ => None,
        }
    }
}

/// Incrementally compresses a payload into the stored layout.
pub struct BlockEncoder {
    pending: Vec<u8>,
    scratch: Vec<u8>,
    lengths: Vec<u32>,
    stored: u64,
    compressed_blocks: usize,
}

impl BlockEncoder {
    pub fn new() -> Self {
        Self {
            pending: Vec::with_capacity(BLOCK_SIZE),
            scratch: vec![0; get_maximum_output_size(BLOCK_SIZE)],
            lengths: Vec::new(),
            stored: 0,
            compressed_blocks: 0,
        }
    }

    /// Appends the stored bytes of every block completed by `data` to `out`.
    pub fn push(&mut self, mut data: &[u8], out: &mut Vec<u8>) {
        if !self.pending.is_empty() {
            let take = (BLOCK_SIZE - self.pending.len()).min(data.len());
            self.pending.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.pending.len() < BLOCK_SIZE {
                return;
            }
            let block = std::mem::take(&mut self.pending);
            self.emit(&block, out);
            self.pending = block;
            self.pending.clear();
        }
        let mut blocks = data.chunks_exact(BLOCK_SIZE);
        for block in &mut blocks {
            self.emit(block, out);
        }
        self.pending.extend_from_slice(blocks.remainder());
    }

    /// Flushes the trailing partial block and the table. Returns the total stored size.
    pub fn finish(&mut self, out: &mut Vec<u8>) -> u64 {
        if !self.pending.is_empty() {
            let block = std::mem::take(&mut self.pending);
            self.emit(&block, out);
        }
        for length in &self.lengths {
            out.extend_from_slice(&length.to_le_bytes());
        }
        out.extend_from_slice(&(self.lengths.len() as u32).to_le_bytes());
        out.extend_from_slice(&MAGIC);
        self.stored + self.lengths.len() as u64 * 4 + FOOTER_SIZE
    }

    fn emit(&mut self, block: &[u8], out: &mut Vec<u8>) {
        // Stop trying once the leading blocks showed the payload is incompressible.
        let probing =
            self.compressed_blocks > 0 || self.lengths.len() < INCOMPRESSIBLE_PROBE_BLOCKS;
        let compressed = probing.then(|| compress_into(block, &mut self.scratch));
        match compressed {
            Some(Ok(n)) if n < block.len() => {
                out.extend_from_slice(&self.scratch[..n]);
                self.lengths.push(n as u32);
                self.stored += n as u64;
                self.compressed_blocks += 1;
            }
            _ => {
                out.extend_from_slice(block);
                self.lengths.push(block.len() as u32 | RAW_FLAG);
                self.stored += block.len() as u64;
            }
        }
    }
}

/// Writes a payload either verbatim or through a [`BlockEncoder`].
pub struct PayloadEncoder {
    encoder: Option<BlockEncoder>,
    scratch: Vec<u8>,
}

impl PayloadEncoder {
    pub fn new(compression: Compression) -> Self {
        Self {
            encoder: (compression == Compression::Lz4).then(BlockEncoder::new),
            scratch: Vec::new(),
        }
    }

    pub async fn write<W: AsyncWrite + Unpin>(
        &mut self,
        writer: &mut W,
        data: &[u8],
    ) -> std::io::Result<()> {
        self.write_while(writer, data, || {}).await
    }

    /// Writes `data`, running `concurrent` (e.g. hashing the same bytes) while the blocks
    /// are compressed on the blocking pool, so the two costs overlap instead of adding up.
    pub async fn write_while<W: AsyncWrite + Unpin>(
        &mut self,
        writer: &mut W,
        data: &[u8],
        concurrent: impl FnOnce(),
    ) -> std::io::Result<()> {
        let Some(mut encoder) = self.encoder.take() else {
            concurrent();
            return writer.write_all(data).await;
        };
        let owned = data.to_vec();
        let mut scratch = std::mem::take(&mut self.scratch);
        let job = tokio::task::spawn_blocking(move || {
            encoder.push(&owned, &mut scratch);
            (encoder, scratch)
        });
        concurrent();
        let (encoder, scratch) = job.await.map_err(std::io::Error::other)?;
        self.encoder = Some(encoder);
        self.scratch = scratch;
        writer.write_all(&self.scratch).await?;
        self.scratch.clear();
        Ok(())
    }

    /// Returns the stored size when the payload was compressed.
    pub async fn finish<W: AsyncWrite + Unpin>(
        &mut self,
        writer: &mut W,
    ) -> std::io::Result<Option<u64>> {
        let Some(encoder) = &mut self.encoder else {
            return Ok(None);
        };
        let stored = encoder.finish(&mut self.scratch);
        writer.write_all(&self.scratch).await?;
        self.scratch.clear();
        Ok(Some(stored))
    }
}

/// Compresses a fully buffered payload. Returns `None` when the payload is too small or
/// does not shrink, in which case the caller stores it raw.
pub fn compress_buffered(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < MIN_COMPRESS_SIZE {
        return None;
    }
    let mut encoder = BlockEncoder::new();
    let mut out = Vec::with_capacity(data.len() / 2);
    encoder.push(data, &mut out);
    encoder.finish(&mut out);
    (out.len() < data.len()).then_some(out)
}

fn corrupt(message: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("corrupt compressed object: {message}"),
    )
}

fn parse_table(tail: &[u8], count: usize) -> Vec<u32> {
    tail[..count * 4]
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
        .collect()
}

fn parse_footer(footer: &[u8]) -> std::io::Result<usize> {
    if footer[4..] != MAGIC {
        return Err(corrupt("bad magic"));
    }
    Ok(u32::from_le_bytes(footer[..4].try_into().unwrap()) as usize)
}

fn plain_block_len(index: usize, plain_size: u64) -> usize {
    (plain_size - index as u64 * BLOCK_SIZE as u64).min(BLOCK_SIZE as u64) as usize
}

fn decode_block(stored: &[u8], length: u32, out: &mut [u8]) -> std::io::Result<()> {
    if length & RAW_FLAG != 0 {
        if stored.len() != out.len() {
            return Err(corrupt("raw block length mismatch"));
        }
        out.copy_from_slice(stored);
        return Ok(());
    }
    match decompress_into(stored, out) {
        Ok(n) if n == out.len() => Ok(()),
        _ => Err(corrupt("block decode failed")),
    }
}

/// Decodes a whole stored payload held in memory.
fn decode_all(stored: &[u8], plain_size: u64) -> std::io::Result<Vec<u8>> {
    if (stored.len() as u64) < FOOTER_SIZE {
        return Err(corrupt("truncated"));
    }
    let count = parse_footer(&stored[stored.len() - FOOTER_SIZE as usize..])?;
    let table_start = (stored.len() - FOOTER_SIZE as usize)
        .checked_sub(count * 4)
        .ok_or_else(|| corrupt("bad table"))?;
    let lengths = parse_table(&stored[table_start..], count);
    let mut plain = vec![0u8; plain_size as usize];
    let mut stored_pos = 0usize;
    for (index, &length) in lengths.iter().enumerate() {
        let stored_len = (length & !RAW_FLAG) as usize;
        let plain_start = index * BLOCK_SIZE;
        let plain_end = plain_start + plain_block_len(index, plain_size);
        let block = stored
            .get(stored_pos..stored_pos + stored_len)
            .ok_or_else(|| corrupt("block out of bounds"))?;
        decode_block(block, length, &mut plain[plain_start..plain_end])?;
        stored_pos += stored_len;
    }
    Ok(plain)
}

/// Opens `length` plaintext bytes starting at `offset` of a compressed object file.
pub async fn open_range(
    path: &Path,
    stored_size: u64,
    plain_size: u64,
    offset: u64,
    length: u64,
) -> Result<ByteStream, StorageError> {
    let mut file = fs::File::open(path).await?;
    if plain_size as usize <= super::blob::SMALL_OBJECT_THRESHOLD as usize {
        let mut stored = Vec::with_capacity(stored_size as usize);
        file.read_to_end(&mut stored).await?;
        let plain = Bytes::from(decode_all(&stored, plain_size)?);
        let end = (offset + length).min(plain_size) as usize;
        return Ok(Box::pin(std::io::Cursor::new(
            plain.slice(offset as usize..end),
        )));
    }

    let mut footer = [0u8; FOOTER_SIZE as usize];
    file.seek(SeekFrom::Start(stored_size - FOOTER_SIZE))
        .await?;
    file.read_exact(&mut footer).await?;
    let count = parse_footer(&footer)?;
    let table_start = (stored_size - FOOTER_SIZE)
        .checked_sub(count as u64 * 4)
        .ok_or_else(|| corrupt("bad table"))?;
    let mut table = vec![0u8; count * 4];
    file.seek(SeekFrom::Start(table_start)).await?;
    file.read_exact(&mut table).await?;
    let lengths = parse_table(&table, count);

    let first_block = (offset / BLOCK_SIZE as u64) as usize;
    let file_offset: u64 = lengths[..first_block]
        .iter()
        .map(|&l| (l & !RAW_FLAG) as u64)
        .sum();
    file.seek(SeekFrom::Start(file_offset)).await?;

    let state = DecodeState {
        file,
        lengths,
        next_block: first_block,
        plain_size,
        skip: (offset % BLOCK_SIZE as u64) as usize,
        remaining: length.min(plain_size.saturating_sub(offset)),
        stored: Vec::new(),
    };
    let blocks = stream::try_unfold(state, |mut state| async move {
        Ok::<_, std::io::Error>(state.next_chunk().await?.map(|chunk| (chunk, state)))
    });
    Ok(Box::pin(StreamReader::new(Box::pin(blocks))))
}

struct DecodeState {
    file: fs::File,
    lengths: Vec<u32>,
    next_block: usize,
    plain_size: u64,
    skip: usize,
    remaining: u64,
    stored: Vec<u8>,
}

impl DecodeState {
    async fn next_chunk(&mut self) -> std::io::Result<Option<Bytes>> {
        if self.remaining == 0 {
            return Ok(None);
        }
        let length = *self
            .lengths
            .get(self.next_block)
            .ok_or_else(|| corrupt("missing block"))?;
        let mut plain = vec![0u8; plain_block_len(self.next_block, self.plain_size)];
        if length & RAW_FLAG != 0 {
            if length & !RAW_FLAG != plain.len() as u32 {
                return Err(corrupt("raw block length mismatch"));
            }
            self.file.read_exact(&mut plain).await?;
        } else {
            self.stored.resize(length as usize, 0);
            self.file.read_exact(&mut self.stored).await?;
            decode_block(&self.stored, length, &mut plain)?;
        }
        self.next_block += 1;

        let end = plain.len().min(self.skip + self.remaining as usize);
        let chunk = Bytes::from(plain).slice(self.skip..end);
        self.remaining -= chunk.len() as u64;
        self.skip = 0;
        Ok(Some(chunk))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(len: usize) -> Vec<u8> {
        (0..len).map(|i| ((i / 7) % 251) as u8).collect()
    }

    fn encode(data: &[u8], chunk: usize) -> Vec<u8> {
        let mut encoder = BlockEncoder::new();
        let mut out = Vec::new();
        for piece in data.chunks(chunk) {
            encoder.push(piece, &mut out);
        }
        let stored = encoder.finish(&mut out);
        assert_eq!(stored, out.len() as u64);
        out
    }

    #[test]
    fn round_trips_across_chunk_sizes() {
        for len in [
            1,
            BLOCK_SIZE - 1,
            BLOCK_SIZE,
            BLOCK_SIZE + 1,
            BLOCK_SIZE * 3 + 17,
        ] {
            let data = sample(len);
            for chunk in [1000, 8192, BLOCK_SIZE, BLOCK_SIZE * 2 + 5] {
                let stored = encode(&data, chunk);
                assert_eq!(decode_all(&stored, len as u64).unwrap(), data);
            }
        }
    }

    #[test]
    fn incompressible_blocks_are_stored_raw() {
        let data: Vec<u8> = (0..BLOCK_SIZE * 2)
            .scan(0x9e3779b9u32, |s, _| {
                *s ^= *s << 13;
                *s ^= *s >> 17;
                *s ^= *s << 5;
                Some(*s as u8)
            })
            .collect();
        let stored = encode(&data, BLOCK_SIZE);
        assert_eq!(decode_all(&stored, data.len() as u64).unwrap(), data);
        assert!(stored.len() <= data.len() + 32);
    }

    #[tokio::test]
    async fn ranges_match_plaintext() {
        let data = sample(BLOCK_SIZE * 3 + 12345);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("obj");
        let stored = encode(&data, 100_000);
        fs::write(&path, &stored).await.unwrap();

        let size = data.len() as u64;
        let cases = [
            (0, size),
            (0, 1),
            (5, 100),
            (BLOCK_SIZE as u64 - 3, 10),
            (BLOCK_SIZE as u64, BLOCK_SIZE as u64),
            (BLOCK_SIZE as u64 + 7, BLOCK_SIZE as u64 * 2),
            (size - 1, 1),
            (size - 500, 500),
        ];
        for (offset, length) in cases {
            let mut reader = open_range(&path, stored.len() as u64, size, offset, length)
                .await
                .unwrap();
            let mut got = Vec::new();
            reader.read_to_end(&mut got).await.unwrap();
            assert_eq!(got, data[offset as usize..(offset + length) as usize]);
        }
    }

    #[test]
    fn rejects_corrupt_footer() {
        let mut stored = encode(&sample(5000), 5000);
        let last = stored.len() - 1;
        stored[last] ^= 0xff;
        assert!(decode_all(&stored, 5000).is_err());
    }
}
