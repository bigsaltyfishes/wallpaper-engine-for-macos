//! On-disk persistence.

pub mod app;
pub mod store;
pub mod wallpaper;
pub mod writer;

pub use app::{
    AppConfig, FilterCfg, GeneralCfg, MonitorCfg, SerializedSelector, UiCfg, WindowGeom,
};
pub use store::{ConfigLoad, ConfigStore};
pub use wallpaper::{AudioCfg, MonitorRender, WallpaperConfig};
