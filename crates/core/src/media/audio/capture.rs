use std::{
    ffi::c_void,
    ptr::{self, NonNull},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

use objc2::{AnyThread, rc::Retained};
use objc2_core_audio::{
    AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart,
    AudioDeviceStop, AudioHardwareCreateAggregateDevice, AudioHardwareCreateProcessTap,
    AudioHardwareDestroyAggregateDevice, AudioHardwareDestroyProcessTap,
    AudioObjectGetPropertyData, AudioObjectID, AudioObjectPropertyAddress, CATapDescription,
    CATapMuteBehavior,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp,
};
use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType,
};
use objc2_foundation::{NSMutableArray, NSNumber, NSString, NSUUID, ns_string};

use super::{
    AudioCaptureBackend, AudioCaptureError, AudioFrameConsumer, AudioResponseController, MonoPcmF32,
};

pub type DefaultAudioResponseController =
    AudioResponseController<crate::WallpaperEngine, PlatformAudioCaptureBackend>;

const NO_ERR: i32 = 0;
const K_AUDIO_OBJECT_UNKNOWN: AudioObjectID = 0;
const K_AUDIO_OBJECT_SYSTEM_OBJECT: AudioObjectID = 1;
const K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN: u32 = 0;
const K_AUDIO_AGGREGATE_DRIFT_COMPENSATION_MEDIUM_QUALITY: u32 = 0x40;
const K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL: u32 = fourcc(*b"glob");
const K_AUDIO_HARDWARE_PROPERTY_TRANSLATE_PID_TO_PROCESS_OBJECT: u32 = fourcc(*b"id2p");
const K_AUDIO_TAP_PROPERTY_FORMAT: u32 = fourcc(*b"tfmt");

// === Three resource types, each with its own Drop ===

struct TapResources {
    id: AudioObjectID,
    uid: Retained<NSString>,
}

impl Drop for TapResources {
    fn drop(&mut self) {
        if self.id != K_AUDIO_OBJECT_UNKNOWN {
            unsafe { AudioHardwareDestroyProcessTap(self.id) };
        }
    }
}

impl TapResources {
    /// Creates a process tap and reads its initial stream format.
    /// Returns the tap plus the sample rate discovered at creation time.
    ///
    /// Retained as a named constructor (rather than inlined into
    /// `CaptureState::start`) because the Core Audio setup is ~100 lines and
    /// combines PID translation, tap description configuration, tap creation,
    /// and format probing; the dedicated name documents the acquisition step.
    #[allow(clippy::single_call_fn)]
    unsafe fn new(
        _consumer: &Arc<dyn AudioFrameConsumer>,
    ) -> Result<(Self, u32), AudioCaptureError> {
        let excluded = {
            let excluded = NSMutableArray::array();
            let process_id = {
                let address = AudioObjectPropertyAddress {
                    mSelector: K_AUDIO_HARDWARE_PROPERTY_TRANSLATE_PID_TO_PROCESS_OBJECT,
                    mScope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
                    mElement: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
                };
                let pid = unsafe { libc::getpid() };
                let mut process_id = K_AUDIO_OBJECT_UNKNOWN;
                let mut size =
                    u32::try_from(std::mem::size_of::<AudioObjectID>()).unwrap_or(u32::MAX);
                let qualifier_size =
                    u32::try_from(std::mem::size_of::<libc::pid_t>()).unwrap_or(u32::MAX);
                let status = unsafe {
                    AudioObjectGetPropertyData(
                        K_AUDIO_OBJECT_SYSTEM_OBJECT,
                        NonNull::from_ref(&address),
                        qualifier_size,
                        (&raw const pid).cast::<c_void>(),
                        NonNull::from_mut(&mut size),
                        NonNull::from_mut(&mut process_id).cast::<c_void>(),
                    )
                };
                if status == NO_ERR {
                    process_id
                } else {
                    K_AUDIO_OBJECT_UNKNOWN
                }
            };
            if process_id != K_AUDIO_OBJECT_UNKNOWN {
                excluded.addObject(&*NSNumber::numberWithUnsignedInt(process_id));
            }
            excluded
        };
        let description = unsafe {
            CATapDescription::initMonoGlobalTapButExcludeProcesses(
                CATapDescription::alloc(),
                &excluded,
            )
        };

        let name = ns_string!("Wallpaper Engine System Audio Tap");
        let uuid = NSUUID::UUID();
        let tap_uid = uuid.UUIDString();

        unsafe {
            description.setPrivate(true);
            description.setMuteBehavior(CATapMuteBehavior::Unmuted);
            description.setName(name);
            description.setUUID(&uuid);
        }

        let mut process_tap_id = K_AUDIO_OBJECT_UNKNOWN;
        let status =
            unsafe { AudioHardwareCreateProcessTap(Some(&description), &raw mut process_tap_id) };
        if status != NO_ERR {
            return Err(status_error(status, "AudioHardwareCreateProcessTap"));
        }

        // Read the tap's stream format to discover the sample rate.
        let address = AudioObjectPropertyAddress {
            mSelector: K_AUDIO_TAP_PROPERTY_FORMAT,
            mScope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
            mElement: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };
        let mut format = unsafe { core::mem::zeroed::<AudioStreamBasicDescription>() };
        let mut size =
            u32::try_from(std::mem::size_of::<AudioStreamBasicDescription>()).unwrap_or(u32::MAX);
        let status = unsafe {
            AudioObjectGetPropertyData(
                process_tap_id,
                NonNull::from_ref(&address),
                0,
                ptr::null(),
                NonNull::from_mut(&mut size),
                NonNull::from_mut(&mut format).cast::<c_void>(),
            )
        };
        if status != NO_ERR || size as usize != std::mem::size_of::<AudioStreamBasicDescription>() {
            // Tap created but format query failed — destroy the tap before returning error.
            unsafe { AudioHardwareDestroyProcessTap(process_tap_id) };
            return Err(AudioCaptureError::Platform(
                "failed to query tap stream format".to_string(),
            ));
        }
        if format.mSampleRate <= 0.0 || format.mSampleRate > f64::from(u32::MAX) {
            unsafe { AudioHardwareDestroyProcessTap(process_tap_id) };
            return Err(AudioCaptureError::Platform(
                "tap reported an invalid sample rate".to_string(),
            ));
        }

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let sample_rate = format.mSampleRate as u32;
        Ok((
            Self {
                id: process_tap_id,
                uid: tap_uid,
            },
            sample_rate,
        ))
    }
}

struct AggregateResources {
    device_id: AudioObjectID,
    #[allow(dead_code)] // Kept alive for potential future use; Drop uses device_id.
    uid: Retained<NSString>,
}

impl Drop for AggregateResources {
    fn drop(&mut self) {
        if self.device_id != K_AUDIO_OBJECT_UNKNOWN {
            unsafe { AudioHardwareDestroyAggregateDevice(self.device_id) };
        }
    }
}

impl AggregateResources {
    #[allow(clippy::single_call_fn)]
    unsafe fn new(tap: &TapResources) -> Result<Self, AudioCaptureError> {
        let uuid = NSUUID::UUID();
        let aggregate_uid = uuid.UUIDString();
        let tap_entry = CFDictionary::from_slices(
            &[
                &*CFString::from_static_str("uid"),
                &*CFString::from_static_str("drift"),
                &*CFString::from_static_str("drift quality"),
            ],
            &[
                AsRef::<CFType>::as_ref(AsRef::<CFString>::as_ref(&tap.uid)),
                AsRef::<CFType>::as_ref(&CFBoolean::new(true)),
                AsRef::<CFType>::as_ref(&CFNumber::new_i32(
                    i32::try_from(K_AUDIO_AGGREGATE_DRIFT_COMPENSATION_MEDIUM_QUALITY)
                        .expect("Core Audio drift-quality constant fits i32"),
                )),
            ],
        );
        let tap_list = CFArray::from_retained_objects(&[tap_entry]);
        let aggregate: CFRetained<CFDictionary> = unsafe {
            core::mem::transmute(CFDictionary::from_slices(
                &[
                    &*CFString::from_static_str("name"),
                    &*CFString::from_static_str("uid"),
                    &*CFString::from_static_str("private"),
                    &*CFString::from_static_str("taps"),
                    &*CFString::from_static_str("tapautostart"),
                ],
                &[
                    AsRef::<CFType>::as_ref(&CFString::from_static_str(
                        "Wallpaper Engine System Audio Capture",
                    )),
                    AsRef::<CFType>::as_ref(AsRef::<CFString>::as_ref(&aggregate_uid)),
                    AsRef::<CFType>::as_ref(&CFBoolean::new(true)),
                    AsRef::<CFType>::as_ref(&tap_list),
                    AsRef::<CFType>::as_ref(&CFBoolean::new(false)),
                ],
            ))
        };

        let mut device_id = K_AUDIO_OBJECT_UNKNOWN;
        let status = unsafe {
            AudioHardwareCreateAggregateDevice(&aggregate, NonNull::from_mut(&mut device_id))
        };
        if status != NO_ERR {
            return Err(status_error(status, "AudioHardwareCreateAggregateDevice"));
        }

        Ok(Self {
            device_id,
            uid: aggregate_uid,
        })
    }
}

struct IoProcResources {
    aggregate_device_id: AudioObjectID,
    id: AudioDeviceIOProcID,
    #[allow(dead_code)] // Prevents deallocation while I/O proc is active.
    callback_state: Box<CallbackState>,
}

impl Drop for IoProcResources {
    fn drop(&mut self) {
        if self.aggregate_device_id != K_AUDIO_OBJECT_UNKNOWN && self.id.is_some() {
            unsafe {
                let _ = AudioDeviceStop(self.aggregate_device_id, self.id);
                let _ = AudioDeviceDestroyIOProcID(self.aggregate_device_id, self.id);
            }
        }
    }
}

impl IoProcResources {
    #[allow(clippy::single_call_fn)]
    unsafe fn new(
        aggregate: &AggregateResources,
        consumer: Arc<dyn AudioFrameConsumer>,
        sample_rate: u32,
    ) -> Result<Self, AudioCaptureError> {
        let callback_state = Box::new(CallbackState {
            consumer,
            mono: Mutex::new(Vec::new()),
            sample_rate: AtomicU32::new(sample_rate),
        });
        let client_data = (&raw const *callback_state).cast_mut().cast::<c_void>();
        let mut io_proc_id = None;
        let status = unsafe {
            AudioDeviceCreateIOProcID(
                aggregate.device_id,
                Some(CallbackState::audio_device_io_proc),
                client_data,
                NonNull::from_mut(&mut io_proc_id),
            )
        };
        if status != NO_ERR {
            return Err(status_error(status, "AudioDeviceCreateIOProcID"));
        }
        Ok(Self {
            aggregate_device_id: aggregate.device_id,
            id: io_proc_id,
            callback_state,
        })
    }
}

// === Refactored CaptureState ===

struct CaptureState {
    consumer: Arc<dyn AudioFrameConsumer>,
    sample_rate: u32,
    running: bool,
    // Drop order (reverse declaration): io_proc → aggregate → tap.
    tap: Option<TapResources>,
    aggregate: Option<AggregateResources>,
    io_proc: Option<IoProcResources>,
}

impl CaptureState {
    fn start(&mut self) -> Result<(), AudioCaptureError> {
        let result = objc2::rc::autoreleasepool(|_| unsafe {
            let (tap, sample_rate) = TapResources::new(&self.consumer)?;
            self.tap = Some(tap);
            self.sample_rate = sample_rate;

            let aggregate = AggregateResources::new(self.tap.as_ref().unwrap())?;
            self.aggregate = Some(aggregate);

            let io_proc = IoProcResources::new(
                self.aggregate.as_ref().unwrap(),
                self.consumer.clone(),
                self.sample_rate,
            )?;
            self.io_proc = Some(io_proc);

            // Start the aggregate device.
            let device_id = self.aggregate.as_ref().unwrap().device_id;
            let io_proc_id = self.io_proc.as_ref().unwrap().id;
            let status = AudioDeviceStart(device_id, io_proc_id);
            if status != NO_ERR {
                return Err(status_error(status, "AudioDeviceStart"));
            }

            self.running = true;
            Ok(())
        });
        if result.is_err() {
            self.stop();
        }
        result
    }

    fn stop(&mut self) {
        self.io_proc = None;
        self.aggregate = None;
        self.tap = None;
        self.running = false;
    }
}

impl Drop for CaptureState {
    fn drop(&mut self) {
        self.stop();
    }
}

// === CallbackState with moved audio_device_io_proc ===

struct CallbackState {
    consumer: Arc<dyn AudioFrameConsumer>,
    mono: Mutex<Vec<f32>>,
    sample_rate: AtomicU32,
}

impl CallbackState {
    /// C ABI I/O proc passed to `AudioDeviceCreateIOProcID`. Kept as a named
    /// associated function because it must cross the FFI boundary as a stable
    /// function pointer — closures cannot be used here.
    #[allow(clippy::single_call_fn)]
    unsafe extern "C-unwind" fn audio_device_io_proc(
        _device: AudioObjectID,
        _now: NonNull<AudioTimeStamp>,
        input_data: NonNull<AudioBufferList>,
        _input_time: NonNull<AudioTimeStamp>,
        _output_data: NonNull<AudioBufferList>,
        _output_time: NonNull<AudioTimeStamp>,
        client_data: *mut c_void,
    ) -> i32 {
        if client_data.is_null() {
            return NO_ERR;
        }

        let state = unsafe { &*client_data.cast::<CallbackState>() };
        let frame_count = {
            let input_data = unsafe { input_data.as_ref() };
            if input_data.mNumberBuffers == 0 {
                return NO_ERR;
            }

            debug_assert!(input_data.mNumberBuffers > 0);
            let first = unsafe { &*input_data.mBuffers.as_ptr() };
            if first.mData.is_null() {
                return NO_ERR;
            }
            let channels = first.mNumberChannels.max(1);
            let bytes_per_frame = usize::try_from(channels)
                .ok()
                .and_then(|channels| std::mem::size_of::<f32>().checked_mul(channels));
            let Some(bytes_per_frame) = bytes_per_frame else {
                return NO_ERR;
            };
            usize::try_from(first.mDataByteSize).unwrap_or(usize::MAX) / bytes_per_frame
        };

        if frame_count == 0 || frame_count > usize::try_from(u32::MAX).expect("u32::MAX fits usize")
        {
            return NO_ERR;
        }

        let Ok(mut mono) = state.mono.lock() else {
            return NO_ERR;
        };

        mono.resize(frame_count, 0.0);
        if unsafe { input_data.as_ref().copy_to_mono_f32(frame_count, &mut mono) }.is_err() {
            return NO_ERR;
        }

        let sample_rate = state.sample_rate.load(Ordering::Relaxed);
        if let Ok(frames) = MonoPcmF32::borrowed(sample_rate, &mono) {
            let _ = state.consumer.submit_mono_audio_frames(frames);
        }

        NO_ERR
    }
}

// === PlatformAudioCaptureBackend ===

pub struct PlatformAudioCaptureBackend {
    state: Option<CaptureState>,
    permission_granted_hint: bool,
}

impl PlatformAudioCaptureBackend {
    /// Creates a platform audio capture backend.
    ///
    /// # Errors
    ///
    /// Currently this constructor does not perform fallible platform setup on
    /// macOS, but the `Result` preserves the public backend API for unsupported
    /// or future platform initialization failures.
    pub fn new() -> Result<Self, AudioCaptureError> {
        Ok(Self {
            state: None,
            permission_granted_hint: false,
        })
    }
}

impl AudioCaptureBackend for PlatformAudioCaptureBackend {
    fn has_permission(&self) -> Result<bool, AudioCaptureError> {
        Ok(self.permission_granted_hint)
    }

    fn request_permission(&mut self) -> Result<bool, AudioCaptureError> {
        self.permission_granted_hint = true;
        Ok(true)
    }

    fn start(&mut self, consumer: Arc<dyn AudioFrameConsumer>) -> Result<(), AudioCaptureError> {
        if self.is_running() {
            return Ok(());
        }

        let mut state = CaptureState {
            consumer,
            sample_rate: 48_000,
            running: false,
            tap: None,
            aggregate: None,
            io_proc: None,
        };
        state.start()?;
        self.permission_granted_hint = true;
        self.state = Some(state);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioCaptureError> {
        let Some(mut state) = self.state.take() else {
            return Ok(());
        };

        state.stop();
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(|state| state.running && state.io_proc.is_some())
    }
}

impl Drop for PlatformAudioCaptureBackend {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

// === Helpers ===

fn status_error(status: i32, operation: &str) -> AudioCaptureError {
    AudioCaptureError::Platform(format!("{operation} failed (OSStatus={status})"))
}

const fn fourcc(bytes: [u8; 4]) -> u32 {
    ((bytes[0] as u32) << 24)
        | ((bytes[1] as u32) << 16)
        | ((bytes[2] as u32) << 8)
        | bytes[3] as u32
}

trait AudioBufferListExt {
    fn buffer_at(&self, index: usize) -> Option<&AudioBuffer>;
    fn buffer_count(&self) -> usize;
    fn copy_to_mono_f32(&self, frame_count: usize, mono: &mut [f32]) -> Result<(), ()>;
}

trait AudioBufferExt {
    fn has_f32_samples(&self, sample_count: usize) -> bool;
}

impl AudioBufferListExt for AudioBufferList {
    fn buffer_at(&self, index: usize) -> Option<&AudioBuffer> {
        if usize::try_from(self.mNumberBuffers).is_ok_and(|count| index < count) {
            Some(unsafe { &*self.mBuffers.as_ptr().add(index) })
        } else {
            None
        }
    }

    fn buffer_count(&self) -> usize {
        usize::try_from(self.mNumberBuffers).unwrap_or(usize::MAX)
    }

    fn copy_to_mono_f32(&self, frame_count: usize, mono: &mut [f32]) -> Result<(), ()> {
        if frame_count == 0 || mono.len() < frame_count || self.buffer_count() == 0 {
            return Err(());
        }

        if self.buffer_count() == 1 {
            let buffer = self.buffer_at(0).unwrap();
            if buffer.mData.is_null() {
                return Err(());
            }

            let channels = usize::try_from(buffer.mNumberChannels.max(1)).unwrap_or(usize::MAX);
            let sample_count = frame_count.checked_mul(channels).ok_or(())?;
            if !buffer.has_f32_samples(sample_count) {
                return Err(());
            }
            let source =
                unsafe { std::slice::from_raw_parts(buffer.mData.cast::<f32>(), sample_count) };
            for (frame, sample) in mono.iter_mut().enumerate().take(frame_count) {
                let base = frame * channels;
                let sum = (0..channels)
                    .map(|channel| source[base + channel])
                    .sum::<f32>();
                #[allow(clippy::cast_precision_loss)]
                let channels = channels as f32;
                *sample = sum / channels;
            }
            return Ok(());
        }

        let mut active_buffers = 0usize;
        mono[..frame_count].fill(0.0);
        for index in 0..self.buffer_count() {
            let Some(buffer) = self.buffer_at(index) else {
                continue;
            };
            if buffer.mData.is_null() || !buffer.has_f32_samples(frame_count) {
                return Err(());
            }
            let source =
                unsafe { std::slice::from_raw_parts(buffer.mData.cast::<f32>(), frame_count) };
            for frame in 0..frame_count {
                mono[frame] += source[frame];
            }
            active_buffers += 1;
        }
        if active_buffers == 0 {
            return Err(());
        }
        for sample in &mut mono[..frame_count] {
            #[allow(clippy::cast_precision_loss)]
            let active_buffer_count = active_buffers as f32;
            *sample /= active_buffer_count;
        }
        Ok(())
    }
}

impl AudioBufferExt for AudioBuffer {
    fn has_f32_samples(&self, sample_count: usize) -> bool {
        let required_bytes = sample_count.checked_mul(std::mem::size_of::<f32>());
        required_bytes.is_some()
            && !self.mData.is_null()
            && usize::try_from(self.mDataByteSize).unwrap_or(usize::MAX) >= required_bytes.unwrap()
    }
}
