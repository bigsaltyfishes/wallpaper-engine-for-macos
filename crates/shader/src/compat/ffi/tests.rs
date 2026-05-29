use std::{
    cell::Cell,
    ffi::{CStr, CString},
    os::raw::{c_char, c_void},
    ptr, slice,
};

use super::{
    RsShaderOwnedBytes, rs_shader_compile_program, rs_shader_last_error,
    rs_shader_program_diagnostics_json, rs_shader_program_free, rs_shader_program_metadata_json,
    rs_shader_program_reflection_json, rs_shader_program_stage_count, rs_shader_program_stage_kind,
    rs_shader_program_stage_spv_word_count, rs_shader_program_stage_spv_words,
};

const SPIRV_MAGIC: u32 = 0x0723_0203;
const VERTEX_SOURCE: &str =
    "attribute vec2 a_Position;\nvoid main() { gl_Position = vec4(a_Position, 0.0, 1.0); }\n";
const FRAGMENT_SOURCE: &str = "void main() { gl_FragColor = vec4(1.0); }\n";
const INCLUDE_VERTEX_SOURCE: &str = "#include \"common/shared.glsl\"\nvoid main() { gl_Position = \
                                     vec4(shared_uv(vec2(0.0)), 0.0, 1.0); }\n";
const INCLUDE_SOURCE: &[u8] = b"vec2 shared_uv(vec2 uv) { return uv; }\n";
const INCLUDE_FIXTURE: ShaderRequestFixture = ShaderRequestFixture {
    shader_name: "ffi/include",
    vertex_source: INCLUDE_VERTEX_SOURCE,
};
#[test]
fn invalid_json_returns_error_and_thread_local_message() {
    let json = CString::new("{").expect("request json should not contain nul");
    let mut program = ptr::null_mut();

    // SAFETY: The request pointer is a valid C string and the out pointer is valid.
    let status = unsafe {
        rs_shader_compile_program(json.as_ptr(), None, ptr::null_mut(), &raw mut program)
    };

    assert_ne!(status, 0);
    assert!(program.is_null());
    let error = last_error();
    assert!(error.contains("json"));
}

#[test]
fn null_pointers_return_errors_without_panicking() {
    let request = CString::new(ShaderRequestFixture::basic().json())
        .expect("request json should not contain nul");

    // SAFETY: This intentionally passes null pointers to verify boundary
    // validation.
    let missing_request =
        unsafe { rs_shader_compile_program(ptr::null(), None, ptr::null_mut(), ptr::null_mut()) };
    // SAFETY: This intentionally passes a null out pointer to verify boundary
    // validation.
    let missing_out = unsafe {
        rs_shader_compile_program(request.as_ptr(), None, ptr::null_mut(), ptr::null_mut())
    };

    assert_ne!(missing_request, 0);
    assert_ne!(missing_out, 0);
    assert!(last_error().contains("out program pointer"));
}

#[test]
fn include_callback_memory_is_freed_once_after_copy() {
    let request =
        CString::new(INCLUDE_FIXTURE.json()).expect("request json should not contain nul");
    let mut program = ptr::null_mut();
    let counters = IncludeCallbackCounters::default();
    let user_data = FreeCallbackUserData::IncludeCounters(&counters);

    // SAFETY: Request and out pointers are valid. The callback returns an owned
    // byte buffer with a matching free callback.
    let status = unsafe {
        rs_shader_compile_program(
            request.as_ptr(),
            Some(IncludeCallbackFixture::read),
            ptr::from_ref(&user_data).cast_mut().cast::<c_void>(),
            &raw mut program,
        )
    };

    assert_eq!(status, 0, "{}", last_error());
    assert_ne!(counters.reads.get(), 0);
    assert_eq!(counters.frees.get(), counters.reads.get());
    assert!(!program.is_null());

    // SAFETY: Program was returned by `rs_shader_compile_program` and has not been
    // freed.
    unsafe { rs_shader_program_free(program) };

    let mut second_program = ptr::null_mut();
    // SAFETY: Request and out pointers are valid. The same callback is reused to
    // verify the callback/free pair is stable across calls.
    let second_status = unsafe {
        rs_shader_compile_program(
            request.as_ptr(),
            Some(IncludeCallbackFixture::read),
            ptr::from_ref(&user_data).cast_mut().cast::<c_void>(),
            &raw mut second_program,
        )
    };
    assert_eq!(second_status, 0, "{}", last_error());
    assert_eq!(counters.frees.get(), counters.reads.get());
    assert!(!second_program.is_null());
    // SAFETY: Program was returned by `rs_shader_compile_program` and has not been
    // freed.
    unsafe { rs_shader_program_free(second_program) };
}

#[test]
fn include_callback_free_reclaims_owned_bytes() {
    let free_count = Cell::new(0usize);
    let source = INCLUDE_SOURCE.to_vec();
    let len = source.len();
    let ptr = Box::into_raw(source.into_boxed_slice()).cast::<u8>();

    // SAFETY: The pointer/length pair comes from the same allocation shape used by
    // the include callback.
    let user_data = FreeCallbackUserData::Counter(&free_count);
    // SAFETY: The pointer/length pair comes from the same allocation shape
    // used by the include callback, and `user_data` remains live here.
    unsafe { free_include_fixture(ptr, len, ptr::from_ref(&user_data).cast_mut().cast()) };

    assert_eq!(free_count.get(), 1);
}

#[test]
fn program_handle_retains_stage_words_and_json_until_free() {
    let request = CString::new(ShaderRequestFixture::basic().json())
        .expect("request json should not contain nul");
    let mut program = ptr::null_mut();

    // SAFETY: Request and out pointers are valid.
    let status = unsafe {
        rs_shader_compile_program(request.as_ptr(), None, ptr::null_mut(), &raw mut program)
    };

    assert_eq!(status, 0, "{}", last_error());
    // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
    assert_eq!(unsafe { rs_shader_program_stage_count(program) }, 2);
    // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
    assert_eq!(unsafe { rs_shader_program_stage_kind(program, 0) }, 0);
    // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
    assert_eq!(unsafe { rs_shader_program_stage_kind(program, 1) }, 1);

    // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
    let words = unsafe { rs_shader_program_stage_spv_words(program, 0) };
    // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
    let count = unsafe { rs_shader_program_stage_spv_word_count(program, 0) };
    assert!(!words.is_null());
    assert!(count > 0);

    // SAFETY: The returned pointer/count pair is borrowed from a live program
    // handle.
    let spirv = unsafe { slice::from_raw_parts(words, count) };
    assert_eq!(spirv.first(), Some(&SPIRV_MAGIC));

    // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
    let metadata = c_str(unsafe { rs_shader_program_metadata_json(program) });
    // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
    let reflection = c_str(unsafe { rs_shader_program_reflection_json(program) });
    // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
    let diagnostics = c_str(unsafe { rs_shader_program_diagnostics_json(program) });

    assert!(metadata.contains("\"active_texture_slots\""));
    assert!(reflection.contains("\"descriptor_bindings\""));
    assert!(diagnostics.contains("\"pass\":\"Legalizer\""));

    // SAFETY: Program was returned by `rs_shader_compile_program` and has not been
    // freed.
    unsafe { rs_shader_program_free(program) };
}

#[test]
fn bridge_json_present_false_marks_texture_slot_absent() {
    let request = CString::new(
        serde_json::json!({
            "shader_name": "ffi/texture-presence",
            "target": "vulkan_spirv",
            "cache_policy": {"mode": "disabled"},
            "stages": [
                {
                    "kind": "vertex",
                    "source": "void main() { gl_Position = vec4(0.0); }\n"
                },
                {
                    "kind": "fragment",
                    "source": concat!(
                        "// [COMBO] {\"combo\":\"LIGHTING\",\"default\":1}\n",
                        "uniform sampler2D g_Texture0;\n",
                        "#if LIGHTING\n",
                        "uniform sampler2D g_Texture1; // {\"combo\":\"NORMALMAP\"}\n",
                        "#endif\n",
                        "void main() {\n",
                        "  vec4 color = texture2D(g_Texture0, vec2(0.5));\n",
                        "#if LIGHTING && NORMALMAP\n",
                        "  color += texture2D(g_Texture1, vec2(0.5));\n",
                        "#endif\n",
                        "  gl_FragColor = color;\n",
                        "}\n",
                    )
                }
            ],
            "combos": [],
            "textures": [
                {"slot": 0, "present": true, "enabled": true, "format": "rgba8"},
                {"slot": 1, "present": false, "enabled": false, "format": "rgba8"}
            ],
            "properties": []
        })
        .to_string(),
    )
    .expect("request json should not contain nul");
    let mut program = ptr::null_mut();

    // SAFETY: Request and out pointers are valid.
    let status = unsafe {
        rs_shader_compile_program(request.as_ptr(), None, ptr::null_mut(), &raw mut program)
    };

    assert_eq!(status, 0, "{}", last_error());
    // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
    let metadata = c_str(unsafe { rs_shader_program_metadata_json(program) });
    // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
    let reflection = c_str(unsafe { rs_shader_program_reflection_json(program) });

    assert!(metadata.contains(r#""name":"NORMALMAP","value":"0""#));
    assert!(!reflection.contains(r#""name":"g_Texture1""#));

    // SAFETY: Program was returned by `rs_shader_compile_program` and has not been
    // freed.
    unsafe { rs_shader_program_free(program) };
}

#[test]
fn json_accessors_return_stable_borrowed_pointers() {
    let request = CString::new(ShaderRequestFixture::basic().json())
        .expect("request json should not contain nul");
    let mut program = ptr::null_mut();

    // SAFETY: Request and out pointers are valid.
    let status = unsafe {
        rs_shader_compile_program(request.as_ptr(), None, ptr::null_mut(), &raw mut program)
    };

    assert_eq!(status, 0, "{}", last_error());
    assert_eq!(
        // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
        unsafe { rs_shader_program_metadata_json(program) },
        // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
        unsafe { rs_shader_program_metadata_json(program) }
    );
    assert_eq!(
        // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
        unsafe { rs_shader_program_reflection_json(program) },
        // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
        unsafe { rs_shader_program_reflection_json(program) }
    );
    assert_eq!(
        // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
        unsafe { rs_shader_program_diagnostics_json(program) },
        // SAFETY: Program is a live handle returned by `rs_shader_compile_program`.
        unsafe { rs_shader_program_diagnostics_json(program) }
    );

    // SAFETY: Program was returned by `rs_shader_compile_program` and has not been
    // freed.
    unsafe { rs_shader_program_free(program) };
}

fn last_error() -> String {
    c_str(rs_shader_last_error())
}

fn c_str(ptr: *const c_char) -> String {
    assert!(!ptr.is_null());
    // SAFETY: Tests only pass pointers returned by this module or valid CString
    // pointers.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

#[derive(Clone, Copy, Debug)]
struct ShaderRequestFixture {
    shader_name: &'static str,
    vertex_source: &'static str,
}

impl ShaderRequestFixture {
    const fn basic() -> Self {
        Self {
            shader_name: "ffi/basic",
            vertex_source: VERTEX_SOURCE,
        }
    }

    fn json(self) -> String {
        serde_json::json!({
            "shader_name": self.shader_name,
            "target": "vulkan_spirv",
            "cache_policy": {"mode": "disabled"},
            "stages": [
                {
                    "kind": "vertex",
                    "source": self.vertex_source
                },
                {
                    "kind": "fragment",
                    "source": FRAGMENT_SOURCE
                }
            ],
            "combos": [],
            "textures": [],
            "properties": []
        })
        .to_string()
    }
}

#[derive(Clone, Copy, Debug)]
struct IncludeCallbackFixture;

#[derive(Debug, Default)]
struct IncludeCallbackCounters {
    reads: Cell<usize>,
    frees: Cell<usize>,
}

impl IncludeCallbackFixture {
    extern "C" fn read(path: *const c_char, user_data: *mut c_void) -> RsShaderOwnedBytes {
        let include_path = c_str(path);
        assert_eq!(include_path, "common/shared.glsl");
        // SAFETY: This fixture is only called with a live
        // `FreeCallbackUserData::IncludeCounters` pointer supplied by the
        // test.
        let user_data_ref = unsafe { &*(user_data.cast::<FreeCallbackUserData<'_>>()) };
        let FreeCallbackUserData::IncludeCounters(counters) = user_data_ref else {
            panic!("include callback received counter-only user data");
        };
        counters.reads.set(counters.reads.get() + 1);
        let source = INCLUDE_SOURCE.to_vec();
        let len = source.len();
        let ptr = Box::into_raw(source.into_boxed_slice()).cast::<u8>();
        RsShaderOwnedBytes {
            ptr,
            len,
            free: Some(free_include_fixture),
            free_user_data: user_data,
        }
    }
}

enum FreeCallbackUserData<'a> {
    IncludeCounters(&'a IncludeCallbackCounters),
    Counter(&'a Cell<usize>),
}

unsafe extern "C" fn free_include_fixture(ptr: *mut u8, len: usize, user_data: *mut c_void) {
    assert!(!ptr.is_null());
    assert!(!user_data.is_null());
    // SAFETY: The callback allocated exactly this pointer and length with
    // Box<[u8]>.
    let bytes = unsafe { slice::from_raw_parts_mut(ptr, len) };
    // SAFETY: The slice was originally allocated as `Box<[u8]>` by the callback.
    let _bytes = unsafe { Box::from_raw(bytes) };
    // SAFETY: Tests pass a live `FreeCallbackUserData` pointer for the
    // duration of each callback/free operation.
    let user_data = unsafe { &*(user_data.cast::<FreeCallbackUserData<'_>>()) };
    match user_data {
        FreeCallbackUserData::IncludeCounters(counters) => {
            counters.frees.set(counters.frees.get() + 1);
        }
        FreeCallbackUserData::Counter(counter) => {
            counter.set(counter.get() + 1);
        }
    }
}
