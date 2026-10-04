use super::AudioLimits;
use crate::archive::MAX_ASSET_BYTES;
use thiserror::Error;

const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const WAVE_FORMAT_ADPCM: u16 = 0x0002;
const WAVE_FORMAT_MPEG: u16 = 0x0055;
const WAVE_FORMAT_IMA_ADPCM: u16 = 0x0011;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xfffe;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WaveError {
    #[error("truncated WAVE source at 0x{offset:X}: need {needed} bytes")]
    Truncated { offset: usize, needed: usize },
    #[error("invalid WAVE source at 0x{offset:X}: {reason}")]
    Invalid { offset: usize, reason: &'static str },
    #[error("unsupported WAVE codec 0x{tag:04X} ({name})")]
    UnsupportedCodec { tag: u16, name: &'static str },
    #[error("unsupported audio container: {0}")]
    UnsupportedContainer(&'static str),
    #[error("unsupported PCM channel count {0}; this decoder accepts mono or stereo")]
    UnsupportedChannels(u16),
    #[error("unsupported PCM sample depth {0}; this decoder accepts 8 or 16 bits")]
    UnsupportedSampleDepth(u16),
    #[error("audio source exceeds the archive asset-byte limit")]
    SourceLimit,
    #[error("invalid audio stream limits")]
    InvalidLimits,
    #[error("WAVE payload contains no complete audio frames")]
    EmptyPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveFormat {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub block_align: u16,
    pub bytes_per_second: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveInfo {
    pub format: WaveFormat,
    pub frame_count: u64,
    pub data_bytes: usize,
    pub(crate) data_offset: usize,
}

/// Validate RIFF chunk bounds and the format fields without allocating from any
/// declared source length. Only classic integer PCM with one or two channels and
/// 8- or 16-bit samples is accepted. Other codecs stay explicit refusals.
pub fn inspect_wave(bytes: &[u8], limits: AudioLimits) -> Result<WaveInfo, WaveError> {
    if limits.chunk_frames == 0 || limits.chunk_frames > super::source::MAX_PCM_CHUNK_FRAMES {
        return Err(WaveError::InvalidLimits);
    }
    if bytes.len() > MAX_ASSET_BYTES as usize {
        return Err(WaveError::SourceLimit);
    }
    if bytes.len() < 12 {
        return Err(WaveError::Truncated {
            offset: 0,
            needed: 12,
        });
    }
    if &bytes[..4] != b"RIFF" {
        if &bytes[..4] == b"RIFX" {
            return Err(WaveError::UnsupportedContainer("big-endian RIFX"));
        }
        if &bytes[..4] == b"RF64" {
            return Err(WaveError::UnsupportedContainer("RF64"));
        }
        if bytes.starts_with(b"OggS") {
            return Err(WaveError::UnsupportedContainer("Ogg"));
        }
        if bytes.starts_with(b"ID3") || is_mpeg_frame(bytes) {
            return Err(WaveError::UnsupportedCodec {
                tag: WAVE_FORMAT_MPEG,
                name: "MPEG audio",
            });
        }
        return Err(WaveError::UnsupportedContainer("unknown audio container"));
    }
    let riff_size = read_u32(bytes, 4)? as usize;
    let riff_end = 8usize.checked_add(riff_size).ok_or(WaveError::Invalid {
        offset: 4,
        reason: "RIFF length overflow",
    })?;
    if riff_size < 4 {
        return Err(WaveError::Invalid {
            offset: 4,
            reason: "RIFF form is shorter than its form tag",
        });
    }
    if riff_end > bytes.len() {
        return Err(WaveError::Truncated {
            offset: bytes.len(),
            needed: riff_end - bytes.len(),
        });
    }
    if riff_end != bytes.len() {
        return Err(WaveError::Invalid {
            offset: riff_end,
            reason: "bytes remain after the declared RIFF form",
        });
    }
    match &bytes[8..12] {
        b"WAVE" => {}
        b"XWMA" => {
            return Err(WaveError::UnsupportedContainer("XWMA RIFF form"));
        }
        _ => return Err(WaveError::UnsupportedContainer("RIFF form is not WAVE")),
    }

    let mut offset = 12usize;
    let mut format = None;
    let mut data = None;
    while offset < riff_end {
        let header_end = offset.checked_add(8).ok_or(WaveError::Invalid {
            offset,
            reason: "chunk header offset overflow",
        })?;
        if header_end > riff_end {
            return Err(WaveError::Truncated {
                offset,
                needed: header_end - riff_end,
            });
        }
        let id = &bytes[offset..offset + 4];
        let length = read_u32(bytes, offset + 4)? as usize;
        let payload_start = header_end;
        let payload_end = payload_start
            .checked_add(length)
            .ok_or(WaveError::Invalid {
                offset: offset + 4,
                reason: "chunk length overflow",
            })?;
        if payload_end > riff_end {
            return Err(WaveError::Invalid {
                offset: offset + 4,
                reason: "declared chunk payload exceeds the RIFF form",
            });
        }
        let padded_end = payload_end
            .checked_add(length & 1)
            .ok_or(WaveError::Invalid {
                offset: offset + 4,
                reason: "chunk padding offset overflow",
            })?;
        if padded_end > riff_end {
            return Err(WaveError::Truncated {
                offset: payload_end,
                needed: padded_end - riff_end,
            });
        }

        if id == b"fmt " {
            if format.is_some() {
                return Err(WaveError::Invalid {
                    offset,
                    reason: "duplicate format chunk",
                });
            }
            format = Some(parse_format(bytes, payload_start, payload_end)?);
        } else if id == b"data" {
            if data.is_some() {
                return Err(WaveError::Invalid {
                    offset,
                    reason: "multiple data chunks are unsupported",
                });
            }
            data = Some((payload_start, length));
        }
        offset = padded_end;
    }
    if offset != riff_end {
        return Err(WaveError::Invalid {
            offset,
            reason: "chunk table did not end on the RIFF boundary",
        });
    }

    let format = format.ok_or(WaveError::Invalid {
        offset: 12,
        reason: "missing format chunk",
    })?;
    let (data_offset, data_bytes) = data.ok_or(WaveError::Invalid {
        offset: 12,
        reason: "missing data chunk",
    })?;
    if data_bytes == 0 {
        return Err(WaveError::EmptyPayload);
    }
    if data_bytes % usize::from(format.block_align) != 0 {
        return Err(WaveError::Invalid {
            offset: data_offset,
            reason: "data length is not a whole number of frames",
        });
    }
    let frame_count =
        u64::try_from(data_bytes / usize::from(format.block_align)).map_err(|_| {
            WaveError::Invalid {
                offset: data_offset,
                reason: "frame count does not fit",
            }
        })?;
    Ok(WaveInfo {
        format,
        frame_count,
        data_bytes,
        data_offset,
    })
}

fn parse_format(bytes: &[u8], start: usize, end: usize) -> Result<WaveFormat, WaveError> {
    let length = end - start;
    if length < 16 {
        return Err(WaveError::Truncated {
            offset: start,
            needed: 16 - length,
        });
    }
    let tag = read_u16(bytes, start)?;
    if tag != WAVE_FORMAT_PCM {
        let name = match tag {
            WAVE_FORMAT_IEEE_FLOAT => "IEEE floating-point WAVE",
            WAVE_FORMAT_ADPCM => "Microsoft ADPCM",
            WAVE_FORMAT_IMA_ADPCM => "IMA ADPCM",
            WAVE_FORMAT_MPEG => "MPEG audio",
            WAVE_FORMAT_EXTENSIBLE => "WAVEFORMATEXTENSIBLE",
            _ => "unrecognized WAVE format tag",
        };
        return Err(WaveError::UnsupportedCodec { tag, name });
    }
    if length == 17 {
        return Err(WaveError::Truncated {
            offset: start + 16,
            needed: 1,
        });
    }
    if length != 16 && length != 18 {
        return Err(WaveError::Invalid {
            offset: start + 16,
            reason: "PCM format extensions are unsupported",
        });
    }
    if length == 18 && read_u16(bytes, start + 16)? != 0 {
        return Err(WaveError::Invalid {
            offset: start + 16,
            reason: "PCM extension size must be zero",
        });
    }

    let channels = read_u16(bytes, start + 2)?;
    if channels == 0 {
        return Err(WaveError::Invalid {
            offset: start + 2,
            reason: "channel count is zero",
        });
    }
    if channels > 2 {
        return Err(WaveError::UnsupportedChannels(channels));
    }
    let sample_rate = read_u32(bytes, start + 4)?;
    if sample_rate == 0 {
        return Err(WaveError::Invalid {
            offset: start + 4,
            reason: "sample rate is zero",
        });
    }
    let bytes_per_second = read_u32(bytes, start + 8)?;
    let block_align = read_u16(bytes, start + 12)?;
    let bits_per_sample = read_u16(bytes, start + 14)?;
    if !matches!(bits_per_sample, 8 | 16) {
        return Err(WaveError::UnsupportedSampleDepth(bits_per_sample));
    }
    let expected_align = channels
        .checked_mul(bits_per_sample / 8)
        .ok_or(WaveError::Invalid {
            offset: start + 12,
            reason: "block alignment overflow",
        })?;
    if block_align != expected_align {
        return Err(WaveError::Invalid {
            offset: start + 12,
            reason: "block alignment disagrees with channels and sample depth",
        });
    }
    let expected_rate = u64::from(sample_rate) * u64::from(block_align);
    if expected_rate > u64::from(u32::MAX) || bytes_per_second != expected_rate as u32 {
        return Err(WaveError::Invalid {
            offset: start + 8,
            reason: "byte rate disagrees with sample rate and block alignment",
        });
    }
    Ok(WaveFormat {
        channels,
        sample_rate,
        bits_per_sample,
        block_align,
        bytes_per_second,
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, WaveError> {
    let end = offset.checked_add(2).ok_or(WaveError::Invalid {
        offset,
        reason: "field offset overflow",
    })?;
    let field = bytes
        .get(offset..end)
        .ok_or(WaveError::Truncated { offset, needed: 2 })?;
    Ok(u16::from_le_bytes([field[0], field[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, WaveError> {
    let end = offset.checked_add(4).ok_or(WaveError::Invalid {
        offset,
        reason: "field offset overflow",
    })?;
    let field = bytes
        .get(offset..end)
        .ok_or(WaveError::Truncated { offset, needed: 4 })?;
    Ok(u32::from_le_bytes([field[0], field[1], field[2], field[3]]))
}

fn is_mpeg_frame(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0
}
