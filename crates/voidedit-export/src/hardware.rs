use windows::core::{Interface, GUID, HSTRING};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

const TICKS_PER_SECOND: i64 = 10_000_000;
const HIGH_PROFILE: u32 = 100;
const MAIN_PROFILE: u32 = 77;

pub struct VideoSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub main_profile: bool,
}

pub struct HardwareWriter {
    writer: IMFSinkWriter,
    video: u32,
    audio: Option<u32>,
    fps: i64,
    frame: i64,
    audio_samples: i64,
    audio_rate: i64,
    buffer: Vec<u8>,
}

pub fn available() -> bool {
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.is_err() {
        return false;
    }
    let output = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_H264 };
    let mut list = std::ptr::null_mut();
    let mut count = 0u32;
    let found = unsafe { MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER, None, Some(&output), &mut list, &mut count) };
    if !list.is_null() {
        unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(list as *const _)) };
    }
    found.is_ok() && count > 0
}

fn media_type(major: &GUID, subtype: &GUID) -> windows::core::Result<IMFMediaType> {
    let kind = unsafe { MFCreateMediaType() }?;
    unsafe {
        kind.SetGUID(&MF_MT_MAJOR_TYPE, major)?;
        kind.SetGUID(&MF_MT_SUBTYPE, subtype)?;
    }
    Ok(kind)
}

fn pack(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

impl HardwareWriter {
    pub fn new(path: &std::path::Path, video: &VideoSettings, audio: Option<(u32, u32)>) -> windows::core::Result<Self> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 2)?;
            let attributes = attributes.ok_or_else(windows::core::Error::empty)?;
            attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            attributes.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1)?;
            let writer = MFCreateSinkWriterFromURL(&HSTRING::from(path.as_os_str()), None, &attributes)?;
            let output = media_type(&MFMediaType_Video, &MFVideoFormat_H264)?;
            output.SetUINT32(&MF_MT_AVG_BITRATE, video.bitrate_kbps.saturating_mul(1000))?;
            output.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            output.SetUINT32(&MF_MT_MPEG2_PROFILE, if video.main_profile { MAIN_PROFILE } else { HIGH_PROFILE })?;
            output.SetUINT64(&MF_MT_FRAME_SIZE, pack(video.width, video.height))?;
            output.SetUINT64(&MF_MT_FRAME_RATE, pack(video.fps, 1))?;
            output.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
            let video_stream = writer.AddStream(&output)?;
            let input = media_type(&MFMediaType_Video, &MFVideoFormat_NV12)?;
            input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            input.SetUINT32(&MF_MT_DEFAULT_STRIDE, video.width)?;
            input.SetUINT64(&MF_MT_FRAME_SIZE, pack(video.width, video.height))?;
            input.SetUINT64(&MF_MT_FRAME_RATE, pack(video.fps, 1))?;
            input.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
            writer.SetInputMediaType(video_stream, &input, None)?;
            let mut audio_stream = None;
            if let Some((rate, kbps)) = audio {
                let output = media_type(&MFMediaType_Audio, &MFAudioFormat_AAC)?;
                output.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                output.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)?;
                output.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
                output.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, supported_aac_bytes(kbps))?;
                let stream = writer.AddStream(&output)?;
                let input = media_type(&MFMediaType_Audio, &MFAudioFormat_PCM)?;
                input.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                input.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)?;
                input.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
                input.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 4)?;
                input.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * 4)?;
                writer.SetInputMediaType(stream, &input, None)?;
                audio_stream = Some(stream);
            }
            writer.BeginWriting()?;
            Ok(Self { writer, video: video_stream, audio: audio_stream, fps: video.fps.max(1) as i64, frame: 0, audio_samples: 0, audio_rate: audio.map_or(48_000, |item| item.0 as i64), buffer: Vec::new() })
        }
    }

    fn write(&self, stream: u32, data: &[u8], time: i64, duration: i64) -> windows::core::Result<()> {
        unsafe {
            let buffer = MFCreateMemoryBuffer(data.len() as u32)?;
            let mut target = std::ptr::null_mut();
            buffer.Lock(&mut target, None, None)?;
            std::ptr::copy_nonoverlapping(data.as_ptr(), target, data.len());
            buffer.Unlock()?;
            buffer.SetCurrentLength(data.len() as u32)?;
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buffer.cast::<IMFMediaBuffer>()?)?;
            sample.SetSampleTime(time)?;
            sample.SetSampleDuration(duration)?;
            self.writer.WriteSample(stream, &sample)
        }
    }

    pub fn video(&mut self, y: &[u8], u: &[u8], v: &[u8]) -> windows::core::Result<()> {
        self.buffer.clear();
        self.buffer.reserve(y.len() + u.len() * 2);
        self.buffer.extend_from_slice(y);
        for (a, b) in u.iter().zip(v) {
            self.buffer.push(*a);
            self.buffer.push(*b);
        }
        let time = self.frame * TICKS_PER_SECOND / self.fps;
        let next = (self.frame + 1) * TICKS_PER_SECOND / self.fps;
        self.frame += 1;
        let data = std::mem::take(&mut self.buffer);
        let result = self.write(self.video, &data, time, next - time);
        self.buffer = data;
        result
    }

    pub fn audio(&mut self, stereo: &[f32]) -> windows::core::Result<()> {
        let Some(stream) = self.audio else { return Ok(()) };
        let mut data = Vec::with_capacity(stereo.len() * 2);
        for sample in stereo {
            data.extend_from_slice(&((sample.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        let count = (stereo.len() / 2) as i64;
        let time = self.audio_samples * TICKS_PER_SECOND / self.audio_rate;
        self.audio_samples += count;
        let next = self.audio_samples * TICKS_PER_SECOND / self.audio_rate;
        self.write(stream, &data, time, next - time)
    }

    pub fn finish(self) -> windows::core::Result<()> {
        unsafe { self.writer.Finalize() }
    }
}

fn supported_aac_bytes(kbps: u32) -> u32 {
    match kbps {
        0..=111 => 12_000,
        112..=143 => 16_000,
        144..=175 => 20_000,
        _ => 24_000,
    }
}
