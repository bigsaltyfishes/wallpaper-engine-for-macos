use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

use objc2_foundation::NSBundle;
use objc2_service_management::{SMAppService, SMAppServiceStatus};

use crate::api::BridgeError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchAtLoginStatus {
    Available { enabled: bool },
    Unavailable,
}

#[derive(Clone)]
pub struct LaunchAtLoginController {
    implementation: Arc<dyn LaunchAtLoginImpl>,
}

trait LaunchAtLoginImpl: Send + Sync {
    fn status(&self) -> Result<LaunchAtLoginStatus, BridgeError>;
    fn set_enabled(&self, enabled: bool) -> Result<LaunchAtLoginStatus, BridgeError>;
}

impl Default for LaunchAtLoginController {
    fn default() -> Self {
        Self {
            implementation: Arc::new(SystemLaunchAtLogin),
        }
    }
}

impl LaunchAtLoginController {
    #[must_use]
    pub fn status(&self) -> LaunchAtLoginStatus {
        self.implementation
            .status()
            .unwrap_or(LaunchAtLoginStatus::Unavailable)
    }

    /// # Errors
    ///
    /// Returns an error when launch at login is unavailable or when the
    /// system service manager rejects the requested state.
    pub fn set_enabled(&self, enabled: bool) -> Result<LaunchAtLoginStatus, BridgeError> {
        self.implementation.set_enabled(enabled)
    }

    #[cfg(test)]
    #[must_use]
    pub fn fake(status: LaunchAtLoginStatus) -> Self {
        Self {
            implementation: Arc::new(FakeLaunchAtLogin {
                status: Mutex::new(status),
            }),
        }
    }
}

struct SystemLaunchAtLogin;

impl LaunchAtLoginImpl for SystemLaunchAtLogin {
    fn status(&self) -> Result<LaunchAtLoginStatus, BridgeError> {
        let bundle = NSBundle::mainBundle();
        let Some(bundle_path) = bundle
            .bundleURL()
            .path()
            .map(|path| std::path::PathBuf::from(path.to_string()))
        else {
            return Ok(LaunchAtLoginStatus::Unavailable);
        };

        let installed_in_system_applications = bundle_path.starts_with("/Applications");
        let installed_in_user_applications =
            dirs::home_dir().is_some_and(|home| bundle_path.starts_with(home.join("Applications")));
        if !installed_in_system_applications && !installed_in_user_applications {
            return Ok(LaunchAtLoginStatus::Unavailable);
        }

        let service = unsafe { SMAppService::mainAppService() };
        let status = unsafe { service.status() };
        Ok(LaunchAtLoginStatus::Available {
            enabled: status == SMAppServiceStatus::Enabled,
        })
    }

    fn set_enabled(&self, enabled: bool) -> Result<LaunchAtLoginStatus, BridgeError> {
        match self.status()? {
            LaunchAtLoginStatus::Available { enabled: current } if current == enabled => {
                return Ok(LaunchAtLoginStatus::Available { enabled });
            }
            LaunchAtLoginStatus::Available { .. } => {}
            LaunchAtLoginStatus::Unavailable => {
                return Err(BridgeError::invalid_input(
                    "launch at login is available only when the app is installed in Applications",
                ));
            }
        }

        let service = unsafe { SMAppService::mainAppService() };
        let result = if enabled {
            unsafe { service.registerAndReturnError() }
        } else {
            unsafe { service.unregisterAndReturnError() }
        };
        result.map_err(|error| BridgeError::engine(error.localizedDescription().to_string()))?;
        self.status()
    }
}

#[cfg(test)]
struct FakeLaunchAtLogin {
    status: Mutex<LaunchAtLoginStatus>,
}

#[cfg(test)]
impl LaunchAtLoginImpl for FakeLaunchAtLogin {
    fn status(&self) -> Result<LaunchAtLoginStatus, BridgeError> {
        Ok(*self.status.lock().expect("fake status lock poisoned"))
    }

    fn set_enabled(&self, enabled: bool) -> Result<LaunchAtLoginStatus, BridgeError> {
        let mut status = self.status.lock().expect("fake status lock poisoned");
        match *status {
            LaunchAtLoginStatus::Available { .. } => {
                *status = LaunchAtLoginStatus::Available { enabled };
                Ok(*status)
            }
            LaunchAtLoginStatus::Unavailable => Err(BridgeError::invalid_input(
                "launch at login is available only when the app is installed in Applications",
            )),
        }
    }
}
