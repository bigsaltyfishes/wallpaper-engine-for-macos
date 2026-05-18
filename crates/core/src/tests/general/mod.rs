pub mod api;
pub mod audio_capture;
pub mod engine_actor;
pub mod platform;
pub mod project;
#[path = "project_smoke.rs"]
mod project_smoke_cases;
crate::tests::macros::general_smoke_cases!(project_smoke, {
    case_3470764447 => crate::tests::general::project_smoke_cases::ProjectManifestCase {
        id: "3470764447",
    },
    case_3177024520 => crate::tests::general::project_smoke_cases::ProjectManifestCase {
        id: "3177024520",
    },
});
#[path = "rendergraph_smoke.rs"]
mod rendergraph_smoke_cases;
crate::tests::macros::general_smoke_cases!(rendergraph_smoke, {
    case_3177024520 => crate::tests::general::rendergraph_smoke_cases::RenderGraphCase {
        id: "3177024520",
    },
    case_3212731906 => crate::tests::general::rendergraph_smoke_cases::RenderGraphCase {
        id: "3212731906",
    },
});
#[path = "resource_smoke.rs"]
mod resource_smoke_cases;
crate::tests::macros::general_smoke_cases!(resource_smoke, {
    case_3470764447 => crate::tests::general::resource_smoke_cases::ResourceCase {
        id: "3470764447",
    },
    case_3177024520 => crate::tests::general::resource_smoke_cases::ResourceCase {
        id: "3177024520",
    },
});
#[path = "runtime_smoke.rs"]
mod runtime_smoke_cases;
crate::tests::macros::general_smoke_cases!(runtime_smoke, {
    case_3470764447 => crate::tests::general::runtime_smoke_cases::RuntimeCase {
        id: "3470764447",
    },
    case_3554183341 => crate::tests::general::runtime_smoke_cases::RuntimeCase {
        id: "3554183341",
    },
});
pub mod scene;
pub mod shader_cache;
pub mod video_codec;
pub mod vulkan;
