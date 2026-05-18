use ash::vk;

pub struct SwapchainSelection {
    pub format: vk::SurfaceFormatKHR,
    pub present_mode: vk::PresentModeKHR,
    pub extent: vk::Extent2D,
    pub image_count: u32,
    pub composite_alpha: vk::CompositeAlphaFlagsKHR,
}

impl SwapchainSelection {
    /// Picks swapchain parameters compatible with the supplied surface
    /// capabilities. Kept as a named method (rather than inlined into
    /// `VulkanRenderSurface` initialization) because the selection has ~50
    /// lines of substantive logic and is covered by future dedicated tests.
    #[allow(clippy::single_call_fn)]
    pub fn select(
        capabilities: vk::SurfaceCapabilitiesKHR,
        formats: &[vk::SurfaceFormatKHR],
        present_modes: &[vk::PresentModeKHR],
        requested_width: u32,
        requested_height: u32,
    ) -> Result<Self, crate::EngineError> {
        if formats.is_empty() {
            return Err(render_error("no Vulkan surface formats were reported"));
        }
        if present_modes.is_empty() {
            return Err(render_error("no Vulkan present modes were reported"));
        }

        let format = formats
            .iter()
            .copied()
            .find(|format| format.format == vk::Format::B8G8R8A8_UNORM)
            .unwrap_or(formats[0]);

        let present_mode = present_modes
            .iter()
            .copied()
            .find(|mode| *mode == vk::PresentModeKHR::MAILBOX)
            .unwrap_or(vk::PresentModeKHR::FIFO);

        let extent = if capabilities.current_extent.width == u32::MAX {
            vk::Extent2D {
                width: requested_width.clamp(
                    capabilities.min_image_extent.width,
                    capabilities.max_image_extent.width,
                ),
                height: requested_height.clamp(
                    capabilities.min_image_extent.height,
                    capabilities.max_image_extent.height,
                ),
            }
        } else {
            capabilities.current_extent
        };

        let mut image_count = capabilities.min_image_count.saturating_add(1);
        if capabilities.max_image_count > 0 {
            image_count = image_count.min(capabilities.max_image_count);
        }

        let composite_alpha = [
            vk::CompositeAlphaFlagsKHR::OPAQUE,
            vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
            vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
            vk::CompositeAlphaFlagsKHR::INHERIT,
        ]
        .into_iter()
        .find(|flag| capabilities.supported_composite_alpha.contains(*flag))
        .unwrap_or(vk::CompositeAlphaFlagsKHR::OPAQUE);

        Ok(Self {
            format,
            present_mode,
            extent,
            image_count,
            composite_alpha,
        })
    }
}

fn render_error(message: impl Into<String>) -> crate::EngineError {
    crate::EngineError::Render(message.into())
}
