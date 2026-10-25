//! Mixing a capture's own sample layout down to the mono `f32` every chunk is, for the captures
//! that get the device's format rather than one they asked for: `mac_capture` (an AVAudioSinkNode
//! hands it an AudioBufferList of float buffers, planar or interleaved) and `windows_capture`
//! when WASAPI refuses to convert to 16 kHz and it records the device's mix format instead (float
//! or integer bytes, any channel count). No device here, so it is tested (`mixdown_tests.rs`) on
//! every platform.

#[cfg(any(target_os = "windows", test))]
/// WAVEFORMATEX's tag for integer PCM.
pub(crate) const WAVE_FORMAT_PCM: u16 = 1;
#[cfg(any(target_os = "windows", test))]
/// WAVEFORMATEX's tag for IEEE float.
pub(crate) const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
#[cfg(any(target_os = "windows", test))]
/// WAVEFORMATEX's tag for WAVEFORMATEXTENSIBLE, whose sub-format says what the samples are.
pub(crate) const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
#[cfg(any(target_os = "windows", test))]
/// KSDATAFORMAT_SUBTYPE_PCM ({00000001-0000-0010-8000-00AA00389B71}) as it lies in memory.
pub(crate) const SUBTYPE_PCM: [u8; 16] = guid_bytes(1);
#[cfg(any(target_os = "windows", test))]
/// KSDATAFORMAT_SUBTYPE_IEEE_FLOAT ({00000003-0000-0010-8000-00AA00389B71}) as it lies in memory.
pub(crate) const SUBTYPE_IEEE_FLOAT: [u8; 16] = guid_bytes(3);

#[cfg(any(target_os = "windows", test))]
/// The KSDATAFORMAT_SUBTYPE GUID for a WAVE_FORMAT tag, in its little-endian memory layout.
const fn guid_bytes(tag: u8) -> [u8; 16] {
    [
        tag, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B,
        0x71,
    ]
}

#[cfg(any(target_os = "windows", test))]
/// How one sample of a device format is stored (little-endian).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SampleKind {
    Float32,
    Int16,
    /// Packed in three bytes.
    Int24,
    /// Also 24 valid bits in a 32-bit container: the low byte is zero.
    Int32,
}

#[cfg(any(target_os = "windows", test))]
impl SampleKind {
    /// Bytes per sample.
    pub(crate) fn size(self) -> usize {
        match self {
            SampleKind::Int16 => 2,
            SampleKind::Int24 => 3,
            SampleKind::Float32 | SampleKind::Int32 => 4,
        }
    }

    fn read(self, bytes: &[u8]) -> f32 {
        match self {
            SampleKind::Float32 => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            SampleKind::Int16 => f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32_768.0,
            SampleKind::Int24 => {
                let value = i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]) >> 8;
                value as f32 / 8_388_608.0
            }
            SampleKind::Int32 => {
                let value = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                (f64::from(value) / 2_147_483_648.0) as f32
            }
        }
    }
}

#[cfg(any(target_os = "windows", test))]
/// The sample kind of a WAVEFORMATEX(TENSIBLE): its tag, its container bit depth and, for an
/// extensible format, its sub-format GUID. `None` for anything else (8-bit, 64-bit float, a
/// compressed or unknown format).
pub(crate) fn sample_kind(
    format_tag: u16,
    bits_per_sample: u16,
    sub_format: Option<[u8; 16]>,
) -> Option<SampleKind> {
    let float = match format_tag {
        WAVE_FORMAT_PCM => false,
        WAVE_FORMAT_IEEE_FLOAT => true,
        WAVE_FORMAT_EXTENSIBLE => match sub_format? {
            SUBTYPE_PCM => false,
            SUBTYPE_IEEE_FLOAT => true,
            _ => return None,
        },
        _ => return None,
    };
    match (float, bits_per_sample) {
        (true, 32) => Some(SampleKind::Float32),
        (false, 16) => Some(SampleKind::Int16),
        (false, 24) => Some(SampleKind::Int24),
        (false, 32) => Some(SampleKind::Int32),
        _ => None,
    }
}

#[cfg(any(target_os = "windows", test))]
/// Interleaved `bytes` of `channels`-channel samples of `kind`, averaged to mono. A partial frame
/// at the end is dropped.
pub(crate) fn mono_from_bytes(bytes: &[u8], kind: SampleKind, channels: usize) -> Vec<f32> {
    if channels == 0 {
        return Vec::new();
    }
    let size = kind.size();
    bytes
        .chunks_exact(size * channels)
        .map(|frame| frame.chunks_exact(size).map(|s| kind.read(s)).sum::<f32>() / channels as f32)
        .collect()
}

#[cfg(any(target_os = "macos", test))]
/// `frames` frames from `buffers`, each interleaved with its own channel count (an
/// AudioBufferList: one buffer per channel when planar, one with every channel when
/// interleaved), averaged over every channel to mono. A buffer shorter than `frames` counts as
/// silence past its end.
pub(crate) fn mono_from_buffers(buffers: &[(&[f32], usize)], frames: usize) -> Vec<f32> {
    let channels: usize = buffers.iter().map(|&(_, channels)| channels).sum();
    let mut mono = vec![0f32; frames];
    if channels == 0 {
        return mono;
    }
    for &(samples, width) in buffers {
        if width == 0 {
            continue;
        }
        for (frame, out) in samples
            .chunks_exact(width)
            .take(frames)
            .zip(mono.iter_mut())
        {
            *out += frame.iter().sum::<f32>();
        }
    }
    mono.iter_mut().for_each(|s| *s /= channels as f32);
    mono
}

#[cfg(test)]
#[path = "mixdown_tests.rs"]
mod tests;
