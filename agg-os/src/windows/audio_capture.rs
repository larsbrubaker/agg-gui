//! The WASAPI side of agg-mic's Windows capture (feature `microphone`): COM
//! on the calling thread ([`ComApartment`]) and a shared-mode capture stream
//! on the default capture endpoint ([`CaptureStream`]), opened in a 16-bit
//! mono PCM format the audio engine converts to, or else in the device's own
//! mix format, and read one packet at a time as bytes.
//!
//! agg-mic (`windows_capture.rs`) is the consumer: it runs the capture
//! thread and its channels, decides whether to fall back to the mix format
//! and whether it can read that format (its `mixdown`), turns the bytes into
//! samples, fills a silent packet, delivers the audio, maps every error to
//! its words, and paces the polling. What needs `unsafe` is here: the COM
//! and WASAPI calls, the read of the mix format `GetMixFormat` allocates (and
//! its `CoTaskMemFree`), and the read of the capture buffer between
//! `GetBuffer` and `ReleaseBuffer`.
//!
//! Soundness: the bytes copied out of a packet are `frames * block_align`,
//! with the block align of the format this stream initialized with, held
//! here, never one the caller supplies; `ReleaseBuffer` is called with the
//! frame count `GetBuffer` returned on every path, a panic while copying
//! included (a guard releases it). The mix format is read only through the
//! pointer `GetMixFormat` returned, its sub-format only when its `cbSize`
//! says the allocation holds it, and it is freed with `CoTaskMemFree` on
//! every path (a guard again), so the caller sees a copy ([`MixFormat`]),
//! never the pointer. COM objects stay on the thread that made them: neither
//! type is `Send` or `Sync`. A [`CaptureStream`] exists only for an apartment
//! that holds its own COM initialization on this thread until it drops:
//! [`CaptureStream::open_default`] refuses a [`ComApartment`] whose
//! `CoInitializeEx` failed (COM there belongs to someone else, who could
//! uninitialize it), before any other COM call, and the stream borrows the
//! apartment, so that apartment's `CoUninitialize` cannot run while the
//! stream's interfaces are alive.

use std::ffi::c_void;
use std::fmt;
use std::marker::PhantomData;

use windows_062::core::HRESULT;
use windows_062::Win32::Foundation::{E_POINTER, E_UNEXPECTED};
use windows_062::Win32::Media::Audio::{
    eCapture, eConsole, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, WAVEFORMATEX, WAVE_FORMAT_PCM,
};
use windows_062::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};

/// WAVEFORMATEX's tag for a WAVEFORMATEXTENSIBLE, which carries a sub-format.
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// The `cbSize` from which a WAVEFORMATEXTENSIBLE holds its SubFormat: the
/// 18-byte WAVEFORMATEX, then Samples (2 bytes) and dwChannelMask (4 bytes),
/// then the 16-byte GUID at byte 24, so 22 bytes past the WAVEFORMATEX.
const EXTENSIBLE_EXTRA_BYTES: u16 = 22;

/// The SubFormat's byte offset in a WAVEFORMATEXTENSIBLE.
const SUB_FORMAT_OFFSET: usize = 24;

/// COM on the calling thread, in the multithreaded apartment:
/// `CoInitializeEx(COINIT_MULTITHREADED)` when made, and `CoUninitialize` on
/// drop if that succeeded (`S_FALSE`, COM already in that apartment here,
/// counts: each success is paired). Neither `Send` nor `Sync`, so it is
/// dropped on the thread that made it.
pub struct ComApartment {
    /// What `CoInitializeEx` returned: `S_OK` and `S_FALSE` are a success.
    result: HRESULT,
    /// Not `Send` or `Sync`: COM's initialization is per thread.
    _thread: PhantomData<*const ()>,
}

impl ComApartment {
    /// Initializes COM on this thread for the multithreaded apartment. A
    /// failure (the thread is already in a single-threaded apartment, say) is
    /// not an error here, but such an apartment holds no COM initialization:
    /// nothing is uninitialized on drop, and [`CaptureStream::open_default`]
    /// refuses it with the `CoInitializeEx` error.
    pub fn multithreaded() -> Self {
        // SAFETY: no reserved pointer; a success is paired with exactly one
        // CoUninitialize, in this value's drop, on this thread (it is not
        // Send).
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        ComApartment {
            result,
            _thread: PhantomData,
        }
    }

    /// Whether `CoInitializeEx` succeeded, so the drop uninitializes.
    pub fn is_initialized(&self) -> bool {
        self.result.is_ok()
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.is_initialized() {
            // SAFETY: pairs the successful CoInitializeEx of `multithreaded`,
            // on the same thread; every CaptureStream borrowing this
            // apartment is gone by now, so no interface it made is alive.
            unsafe { CoUninitialize() };
        }
    }
}

/// A failed COM or WASAPI call: its HRESULT and the system's message for it
/// (`windows::core::Error::message`, taken when the call failed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureError {
    code: i32,
    message: String,
}

impl CaptureError {
    /// The HRESULT, as its signed 32-bit value (`0x8007_0005_u32 as i32` is
    /// `E_ACCESSDENIED`).
    pub fn code(&self) -> i32 {
        self.code
    }

    /// The system's message for the error.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl CaptureError {
    /// A failed call's error, its message taken now. Private, so no
    /// `windows` type is in this module's API.
    fn of(error: windows_062::core::Error) -> Self {
        CaptureError {
            code: error.code().0,
            message: error.message(),
        }
    }

    /// The error for `code`, with the system's message for it.
    fn of_code(code: HRESULT) -> Self {
        CaptureError::of(windows_062::core::Error::from(code))
    }
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (HRESULT {:#010x})", self.message, self.code)
    }
}

impl std::error::Error for CaptureError {}

/// The device's mix format, as `GetMixFormat` gave it: the fields of its
/// WAVEFORMATEX, and the SubFormat GUID's 16 bytes as they lie in memory
/// when it is a WAVEFORMATEXTENSIBLE (tag `0xFFFE`) that holds one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixFormat {
    /// `wFormatTag`.
    pub format_tag: u16,
    /// `nChannels`.
    pub channels: u16,
    /// `nSamplesPerSec`.
    pub samples_per_sec: u32,
    /// `nBlockAlign`: bytes per frame.
    pub block_align: u16,
    /// `wBitsPerSample`: the container's bits per sample.
    pub bits_per_sample: u16,
    /// The SubFormat, for an extensible format whose `cbSize` is at least 22.
    pub sub_format: Option<[u8; 16]>,
}

/// What a [`CaptureStream`] captures in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureFormat {
    /// 16-bit mono integer PCM at the rate [`CaptureStream::open_default`]
    /// was given, converted by the audio engine: 2 bytes per frame.
    Converted,
    /// The device's own mix format.
    Mix(MixFormat),
}

/// Why [`CaptureStream::open_default`] opened nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// A COM or WASAPI call failed: initializing COM (the apartment's
    /// `CoInitializeEx`, so no other call was made), creating the device
    /// enumerator, getting
    /// the default capture endpoint, activating a client, initializing it
    /// (in the converted format when the caller did not fall back, or in the
    /// mix format), reading the mix format, getting the capture client, or
    /// starting it.
    Call(CaptureError),
    /// The caller's `accept_mix` refused the device's mix format; nothing
    /// was initialized with it.
    MixRejected(MixFormat),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Call(error) => write!(f, "{error}"),
            Self::MixRejected(format) => write!(
                f,
                "the mix format (tag {:#x}, {} bits) was not accepted",
                format.format_tag, format.bits_per_sample
            ),
        }
    }
}

impl std::error::Error for OpenError {}

/// A failed call's [`OpenError`].
fn call_error(error: windows_062::core::Error) -> OpenError {
    OpenError::Call(CaptureError::of(error))
}

/// One packet [`CaptureStream::next_packet`] took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet {
    /// The frames `GetBuffer` returned.
    pub frames: u32,
    /// The engine flagged the packet silent (`AUDCLNT_BUFFERFLAGS_SILENT`)
    /// or gave no data pointer: its bytes were not copied, and the caller
    /// treats the packet as `frames` frames of silence.
    pub silent: bool,
}

/// An initialized, started shared-mode capture on the default capture
/// endpoint (eCapture, eConsole). It borrows the [`ComApartment`] of its
/// thread, one whose `CoInitializeEx` succeeded, so it cannot outlive the
/// COM initialization that apartment holds, and it is neither `Send` nor
/// `Sync`. Dropping it calls `IAudioClient::Stop`, then releases the client
/// and then the capture client.
pub struct CaptureStream<'com> {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    /// Bytes per frame of the format the client was initialized with.
    block_align: usize,
    format: CaptureFormat,
    /// Ties the stream to its thread's COM; `&ComApartment` is not `Send`.
    _com: PhantomData<&'com ComApartment>,
}

impl<'com> CaptureStream<'com> {
    /// Opens the default capture endpoint in shared mode with a buffer of
    /// `buffer_duration_hns` (100 ns units) and starts it.
    ///
    /// First as 16-bit mono PCM at `rate` Hz, which the audio engine
    /// converts to (`AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM |
    /// AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY`). If that `Initialize` fails,
    /// `fall_back` hears why: when it returns false, that is the error; when
    /// it returns true, a fresh client (one whose `Initialize` failed is not
    /// reused) reads the device's mix format, `accept_mix` says whether the
    /// caller can read it, and if so the client is initialized with it (no
    /// stream flags). The calls, in order: `CoCreateInstance` of the device
    /// enumerator, `GetDefaultAudioEndpoint`, `Activate`, `Initialize`, then
    /// for the mix format `Activate`, `GetMixFormat`, `Initialize`,
    /// `CoTaskMemFree`; then `GetService` for the capture client and `Start`.
    ///
    /// An apartment whose `CoInitializeEx` failed is refused first, with that
    /// error, before any of these calls and before either closure is called.
    pub fn open_default(
        com: &'com ComApartment,
        rate: u32,
        buffer_duration_hns: i64,
        fall_back: impl FnOnce(&CaptureError) -> bool,
        accept_mix: impl FnOnce(&MixFormat) -> bool,
    ) -> Result<Self, OpenError> {
        // COM on this thread that the apartment does not hold could be
        // uninitialized by its owner while the stream is alive.
        if !com.is_initialized() {
            return Err(OpenError::Call(CaptureError::of_code(com.result)));
        }
        // SAFETY: plain WASAPI calls on this thread, whose COM `com` holds
        // initialized (its CoInitializeEx succeeded, checked above) until it
        // drops, which the stream's borrow keeps after the stream;
        // `converted` outlives the Initialize it is passed to, and the mix
        // format is handled by `initialize_mix_format`.
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(call_error)?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eCapture, eConsole)
                .map_err(call_error)?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(call_error)?;
            let converted = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM as u16,
                nChannels: 1,
                nSamplesPerSec: rate,
                // 2 bytes a frame; a rate past u32::MAX / 2 saturates, and
                // the engine refuses it.
                nAvgBytesPerSec: rate.saturating_mul(2),
                nBlockAlign: 2,
                wBitsPerSample: 16,
                cbSize: 0,
            };
            let initialized = client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                buffer_duration_hns,
                0,
                &converted,
                None,
            );
            let (client, format) = match initialized {
                Ok(()) => (client, CaptureFormat::Converted),
                Err(error) => {
                    let error = CaptureError::of(error);
                    if !fall_back(&error) {
                        return Err(OpenError::Call(error));
                    }
                    // A fresh client: one whose Initialize failed is not reused.
                    let client: IAudioClient =
                        device.Activate(CLSCTX_ALL, None).map_err(call_error)?;
                    let mix = initialize_mix_format(&client, buffer_duration_hns, accept_mix)?;
                    (client, CaptureFormat::Mix(mix))
                }
            };
            let capture: IAudioCaptureClient = client.GetService().map_err(call_error)?;
            client.Start().map_err(call_error)?;
            let block_align = match format {
                CaptureFormat::Converted => usize::from(converted.nBlockAlign),
                CaptureFormat::Mix(mix) => usize::from(mix.block_align),
            };
            Ok(CaptureStream {
                client,
                capture,
                block_align,
                format,
                _com: PhantomData,
            })
        }
    }

    /// What the stream captures in.
    pub fn format(&self) -> CaptureFormat {
        self.format
    }

    /// Takes the next packet, if one is waiting (`GetNextPacketSize` > 0):
    /// `GetBuffer`, then, unless the packet is silent, appends its
    /// `frames * block_align` bytes to `into`, then `ReleaseBuffer` of the
    /// same frame count. `Ok(None)` when no packet is waiting.
    pub fn next_packet(&self, into: &mut Vec<u8>) -> Result<Option<Packet>, CaptureError> {
        // SAFETY: a plain call on the capture client, on its COM thread.
        if unsafe { self.capture.GetNextPacketSize() }.map_err(CaptureError::of)? == 0 {
            return Ok(None);
        }
        let mut data = std::ptr::null_mut();
        let mut frames = 0u32;
        let mut flags = 0u32;
        // SAFETY: the three out-pointers point at the locals above, which
        // outlive the call; the two optional positions are not asked for.
        unsafe {
            self.capture
                .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                .map_err(CaptureError::of)?;
        }
        // From here the buffer is held: released with `frames` on every path.
        let held = HeldBuffer {
            capture: &self.capture,
            frames,
        };
        let silent = flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null();
        if !silent {
            let Some(len) = (frames as usize).checked_mul(self.block_align) else {
                // More bytes than the address space: no buffer is that big.
                held.release().map_err(CaptureError::of)?;
                return Err(CaptureError::of_code(E_UNEXPECTED));
            };
            // SAFETY: GetBuffer succeeded with a non-null `data`, which WASAPI
            // documents as `frames` frames of the stream's format, each
            // `block_align` bytes (the format this stream initialized with),
            // readable until ReleaseBuffer, which `held` calls only after this
            // copy (or while unwinding from it).
            let bytes = unsafe { std::slice::from_raw_parts(data, len) };
            into.extend_from_slice(bytes);
        }
        held.release().map_err(CaptureError::of)?;
        Ok(Some(Packet { frames, silent }))
    }
}

impl Drop for CaptureStream<'_> {
    fn drop(&mut self) {
        // SAFETY: the client was started by `open_default` on this thread,
        // and only for an apartment whose own CoInitializeEx succeeded; that
        // apartment, borrowed, has not dropped, so its COM initialization
        // still holds.
        let _ = unsafe { self.client.Stop() };
    }
}

/// A packet `GetBuffer` handed out: [`HeldBuffer::release`] releases it,
/// and dropping it unreleased (a panic while copying) releases it too.
struct HeldBuffer<'a> {
    capture: &'a IAudioCaptureClient,
    frames: u32,
}

impl HeldBuffer<'_> {
    /// `ReleaseBuffer` of the frames `GetBuffer` returned, with its result.
    fn release(self) -> windows_062::core::Result<()> {
        let held = std::mem::ManuallyDrop::new(self);
        // SAFETY: pairs the successful GetBuffer that made `held`, with the
        // frame count it returned; the drop, which would release it again,
        // does not run.
        unsafe { held.capture.ReleaseBuffer(held.frames) }
    }
}

impl Drop for HeldBuffer<'_> {
    fn drop(&mut self) {
        // SAFETY: as in `release`, which did not run: this is the only
        // release of that GetBuffer.
        let _ = unsafe { self.capture.ReleaseBuffer(self.frames) };
    }
}

/// The mix format `GetMixFormat` allocated, freed with `CoTaskMemFree` when
/// dropped.
struct MixFormatMemory(*mut WAVEFORMATEX);

impl Drop for MixFormatMemory {
    fn drop(&mut self) {
        // SAFETY: the pointer came from GetMixFormat, which allocates it with
        // CoTaskMemAlloc, and is freed only here.
        unsafe { CoTaskMemFree(Some(self.0 as *const c_void)) };
    }
}

/// Initializes `client` in shared mode with the device's own mix format, if
/// `accept_mix` accepts it, and returns that format. The memory
/// `GetMixFormat` returns is freed on every path, after the `Initialize`.
///
/// # Safety
///
/// Called on a COM-initialized thread with a client that is not initialized
/// yet.
unsafe fn initialize_mix_format(
    client: &IAudioClient,
    buffer_duration_hns: i64,
    accept_mix: impl FnOnce(&MixFormat) -> bool,
) -> Result<MixFormat, OpenError> {
    // SAFETY: a plain call on a client on its COM thread (the caller's
    // contract).
    let mix = unsafe { client.GetMixFormat() }.map_err(call_error)?;
    if mix.is_null() {
        return Err(OpenError::Call(CaptureError::of_code(E_POINTER)));
    }
    let memory = MixFormatMemory(mix);
    // SAFETY: GetMixFormat returned a non-null WAVEFORMATEX (or a
    // WAVEFORMATEXTENSIBLE, whose `cbSize` says so); it is packed, so it is
    // read unaligned. `memory` frees it only when this function returns.
    let header = unsafe { std::ptr::read_unaligned(mix) };
    let sub_format = (header.wFormatTag == WAVE_FORMAT_EXTENSIBLE
        && header.cbSize >= EXTENSIBLE_EXTRA_BYTES)
        .then(|| {
            // SAFETY: a `cbSize` of at least 22 means the allocation holds
            // the 18-byte WAVEFORMATEX and 22 bytes after it, so the 16
            // bytes at offset 24 are in it; read unaligned, as bytes.
            unsafe {
                std::ptr::read_unaligned(
                    (mix as *const u8).add(SUB_FORMAT_OFFSET) as *const [u8; 16]
                )
            }
        });
    let format = MixFormat {
        format_tag: header.wFormatTag,
        channels: header.nChannels,
        samples_per_sec: header.nSamplesPerSec,
        block_align: header.nBlockAlign,
        bits_per_sample: header.wBitsPerSample,
        sub_format,
    };
    let initialized = if accept_mix(&format) {
        // SAFETY: the mix format, still allocated (`memory` is alive), is
        // passed to Initialize, which only reads it during the call.
        unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                0,
                buffer_duration_hns,
                0,
                mix,
                None,
            )
        }
        .map_err(call_error)
    } else {
        Err(OpenError::MixRejected(format))
    };
    drop(memory);
    initialized.map(|()| format)
}
