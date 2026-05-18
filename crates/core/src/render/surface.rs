use std::ffi::{CStr, CString, c_void};

use ash::vk;

use super::swapchain;

pub struct RenderSurfaceConfig {
    metal_layer: *mut c_void,
    width: u32,
    height: u32,
}

impl RenderSurfaceConfig {
    #[must_use]
    pub fn builder(metal_layer: *mut c_void) -> RenderSurfaceConfigBuilder {
        RenderSurfaceConfigBuilder {
            metal_layer,
            width: None,
            height: None,
        }
    }
}

pub struct RenderSurfaceConfigBuilder {
    metal_layer: *mut c_void,
    width: Option<u32>,
    height: Option<u32>,
}

impl RenderSurfaceConfigBuilder {
    #[must_use]
    pub fn extent(mut self, width: u32, height: u32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    /// # Errors
    ///
    /// Returns an error if the Metal layer pointer is null or dimensions are
    /// missing/zero.
    pub fn build(self) -> Result<RenderSurfaceConfig, crate::EngineError> {
        if self.metal_layer.is_null() {
            return Err(invalid_input("metal layer handle must not be null"));
        }

        let width = self
            .width
            .ok_or_else(|| invalid_input("surface width must be specified"))?;
        let height = self
            .height
            .ok_or_else(|| invalid_input("surface height must be specified"))?;

        if width == 0 || height == 0 {
            return Err(invalid_input("swapchain dimensions must be non-zero"));
        }

        Ok(RenderSurfaceConfig {
            metal_layer: self.metal_layer,
            width,
            height,
        })
    }
}

struct VulkanInstance {
    entry: ash::Entry,
    instance: ash::Instance,
    surface_loader: ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
}

impl VulkanInstance {
    #[allow(clippy::single_call_fn)]
    unsafe fn new(config: &RenderSurfaceConfig) -> Result<Self, crate::EngineError> {
        let entry = unsafe { ash::Entry::load().unwrap() };

        let instance_extensions = unsafe { entry.enumerate_instance_extension_properties(None) }
            .map_err(|error| vk_error("vkEnumerateInstanceExtensionProperties", error))?;

        require_extension(
            &instance_extensions,
            ash::khr::surface::NAME,
            "VK_KHR_surface is not available from the current Vulkan loader",
        )?;
        require_extension(
            &instance_extensions,
            ash::ext::metal_surface::NAME,
            "VK_EXT_metal_surface is not available from the current Vulkan loader",
        )?;

        let mut enabled_instance_extensions = vec![
            ash::khr::surface::NAME.as_ptr(),
            ash::ext::metal_surface::NAME.as_ptr(),
        ];
        let mut instance_flags = vk::InstanceCreateFlags::empty();
        if has_extension(
            &instance_extensions,
            ash::khr::portability_enumeration::NAME,
        ) {
            enabled_instance_extensions.push(ash::khr::portability_enumeration::NAME.as_ptr());
            instance_flags |= vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR;
        }

        let application_name = CString::new("wallpaper_core_vulkan_surface")
            .expect("static application name has no nul bytes");
        let engine_name =
            CString::new("wallpaper_core").expect("static engine name has no nul bytes");
        let application_info = vk::ApplicationInfo::default()
            .application_name(&application_name)
            .application_version(vk::make_api_version(0, 0, 1, 0))
            .engine_name(&engine_name)
            .engine_version(vk::make_api_version(0, 0, 1, 0))
            .api_version(vk::API_VERSION_1_1);
        let instance_create_info = vk::InstanceCreateInfo::default()
            .flags(instance_flags)
            .application_info(&application_info)
            .enabled_extension_names(&enabled_instance_extensions);

        let instance = unsafe { entry.create_instance(&instance_create_info, None) }
            .map_err(|error| vk_error("vkCreateInstance", error))?;

        let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);
        let metal_surface_loader = ash::ext::metal_surface::Instance::new(&entry, &instance);

        let metal_surface_create_info = vk::MetalSurfaceCreateInfoEXT::default()
            .layer(config.metal_layer.cast::<vk::CAMetalLayer>());
        let surface =
            unsafe { metal_surface_loader.create_metal_surface(&metal_surface_create_info, None) }
                .map_err(|error| vk_error("vkCreateMetalSurfaceEXT", error))?;

        Ok(Self {
            entry,
            instance,
            surface_loader,
            surface,
        })
    }
}

impl Drop for VulkanInstance {
    fn drop(&mut self) {
        unsafe {
            self.surface_loader.destroy_surface(self.surface, None);
            self.instance.destroy_instance(None);
        }
        let _ = &self.entry;
    }
}

struct VulkanDevice {
    physical_device: vk::PhysicalDevice,
    queue_family_index: u32,
    device: ash::Device,
    swapchain_loader: ash::khr::swapchain::Device,
    graphics_queue: vk::Queue,
    present_queue: vk::Queue,
}

impl VulkanDevice {
    #[allow(clippy::single_call_fn)]
    unsafe fn new(instance: &VulkanInstance) -> Result<Self, crate::EngineError> {
        let physical_devices = unsafe { instance.instance.enumerate_physical_devices() }
            .map_err(|error| vk_error("vkEnumeratePhysicalDevices", error))?;
        if physical_devices.is_empty() {
            return Err(render_error("no Vulkan physical devices were found"));
        }

        let mut result = None;
        'out: for physical_device in physical_devices {
            let queue_families = unsafe {
                instance
                    .instance
                    .get_physical_device_queue_family_properties(physical_device)
            };
            for (index, queue_family) in queue_families.iter().enumerate() {
                let queue_family_index = u32::try_from(index)
                    .map_err(|_| render_error("Vulkan queue family index exceeds u32"))?;
                let supports_present = unsafe {
                    instance.surface_loader.get_physical_device_surface_support(
                        physical_device,
                        queue_family_index,
                        instance.surface,
                    )
                }
                .map_err(|error| vk_error("vkGetPhysicalDeviceSurfaceSupportKHR", error))?;

                if queue_family.queue_flags.contains(vk::QueueFlags::GRAPHICS) && supports_present {
                    result = Some((physical_device, queue_family_index));
                    break 'out;
                }
            }
        }

        let (physical_device, queue_family_index) = result.ok_or(render_error(
            "no Vulkan queue family supports both graphics and present for the Metal surface",
        ))?;

        let device_extensions = unsafe {
            instance
                .instance
                .enumerate_device_extension_properties(physical_device)
        }
        .map_err(|error| vk_error("vkEnumerateDeviceExtensionProperties", error))?;
        require_extension(
            &device_extensions,
            ash::khr::swapchain::NAME,
            "VK_KHR_swapchain is not available on the selected Vulkan device",
        )?;

        let mut enabled_device_extensions = vec![ash::khr::swapchain::NAME.as_ptr()];
        if has_extension(&device_extensions, ash::khr::portability_subset::NAME) {
            enabled_device_extensions.push(ash::khr::portability_subset::NAME.as_ptr());
        }

        let queue_priority = [1.0_f32];
        let queue_create_info = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&queue_priority)];
        let device_create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_create_info)
            .enabled_extension_names(&enabled_device_extensions);
        let device = unsafe {
            instance
                .instance
                .create_device(physical_device, &device_create_info, None)
        }
        .map_err(|error| vk_error("vkCreateDevice", error))?;

        let swapchain_loader = ash::khr::swapchain::Device::new(&instance.instance, &device);
        let graphics_queue = unsafe { device.get_device_queue(queue_family_index, 0) };
        let present_queue = graphics_queue;

        Ok(Self {
            physical_device,
            queue_family_index,
            device,
            swapchain_loader,
            graphics_queue,
            present_queue,
        })
    }
}

impl Drop for VulkanDevice {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_device(None);
        }
    }
}

struct VulkanSwapchain {
    swapchain_loader: ash::khr::swapchain::Device,
    swapchain: vk::SwapchainKHR,
    images: Vec<vk::Image>,
    image_initialized: Vec<bool>,
}

impl VulkanSwapchain {
    #[allow(clippy::single_call_fn)]
    unsafe fn new(
        instance: &VulkanInstance,
        device: &VulkanDevice,
        config: &RenderSurfaceConfig,
    ) -> Result<Self, crate::EngineError> {
        let surface_capabilities = unsafe {
            instance
                .surface_loader
                .get_physical_device_surface_capabilities(device.physical_device, instance.surface)
        }
        .map_err(|error| vk_error("vkGetPhysicalDeviceSurfaceCapabilitiesKHR", error))?;
        let surface_formats = unsafe {
            instance
                .surface_loader
                .get_physical_device_surface_formats(device.physical_device, instance.surface)
        }
        .map_err(|error| vk_error("vkGetPhysicalDeviceSurfaceFormatsKHR", error))?;
        let present_modes = unsafe {
            instance
                .surface_loader
                .get_physical_device_surface_present_modes(device.physical_device, instance.surface)
        }
        .map_err(|error| vk_error("vkGetPhysicalDeviceSurfacePresentModesKHR", error))?;

        let swapchain_selection = swapchain::SwapchainSelection::select(
            surface_capabilities,
            &surface_formats,
            &present_modes,
            config.width,
            config.height,
        )?;

        let swapchain_create_info = vk::SwapchainCreateInfoKHR::default()
            .surface(instance.surface)
            .min_image_count(swapchain_selection.image_count)
            .image_format(swapchain_selection.format.format)
            .image_color_space(swapchain_selection.format.color_space)
            .image_extent(swapchain_selection.extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(surface_capabilities.current_transform)
            .composite_alpha(swapchain_selection.composite_alpha)
            .present_mode(swapchain_selection.present_mode)
            .clipped(true);

        let swapchain_loader = device.swapchain_loader.clone();
        let swapchain = unsafe { swapchain_loader.create_swapchain(&swapchain_create_info, None) }
            .map_err(|error| vk_error("vkCreateSwapchainKHR", error))?;
        let images = unsafe { swapchain_loader.get_swapchain_images(swapchain) }
            .map_err(|error| vk_error("vkGetSwapchainImagesKHR", error))?;
        if images.is_empty() {
            return Err(render_error("swapchain returned zero images"));
        }
        let image_initialized = vec![false; images.len()];

        Ok(Self {
            swapchain_loader,
            swapchain,
            images,
            image_initialized,
        })
    }
}

impl Drop for VulkanSwapchain {
    fn drop(&mut self) {
        unsafe {
            self.swapchain_loader
                .destroy_swapchain(self.swapchain, None);
        }
    }
}

struct VulkanFrameSync {
    device: ash::Device,
    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
    image_available_semaphore: vk::Semaphore,
    render_finished_semaphore: vk::Semaphore,
    in_flight_fence: vk::Fence,
}

impl VulkanFrameSync {
    #[allow(clippy::single_call_fn)]
    unsafe fn new(device: &VulkanDevice) -> Result<Self, crate::EngineError> {
        let command_pool_create_info = vk::CommandPoolCreateInfo::default()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(device.queue_family_index);
        let command_pool = unsafe {
            device
                .device
                .create_command_pool(&command_pool_create_info, None)
        }
        .map_err(|error| vk_error("vkCreateCommandPool", error))?;

        let command_buffer_allocate_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let command_buffers = unsafe {
            device
                .device
                .allocate_command_buffers(&command_buffer_allocate_info)
        }
        .map_err(|error| vk_error("vkAllocateCommandBuffers", error))?;
        let command_buffer = command_buffers
            .first()
            .copied()
            .ok_or_else(|| render_error("vkAllocateCommandBuffers returned no buffers"))?;

        let semaphore_create_info = vk::SemaphoreCreateInfo::default();
        let image_available_semaphore =
            unsafe { device.device.create_semaphore(&semaphore_create_info, None) }
                .map_err(|error| vk_error("vkCreateSemaphore(image_available)", error))?;
        let render_finished_semaphore =
            unsafe { device.device.create_semaphore(&semaphore_create_info, None) }
                .map_err(|error| vk_error("vkCreateSemaphore(render_finished)", error))?;

        let fence_create_info =
            vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
        let in_flight_fence = unsafe { device.device.create_fence(&fence_create_info, None) }
            .map_err(|error| vk_error("vkCreateFence", error))?;

        Ok(Self {
            device: device.device.clone(),
            command_pool,
            command_buffer,
            image_available_semaphore,
            render_finished_semaphore,
            in_flight_fence,
        })
    }
}

impl Drop for VulkanFrameSync {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_fence(self.in_flight_fence, None);
            self.device
                .destroy_semaphore(self.render_finished_semaphore, None);
            self.device
                .destroy_semaphore(self.image_available_semaphore, None);
            self.device.destroy_command_pool(self.command_pool, None);
        }
    }
}

pub struct VulkanRenderSurface {
    // Field declaration order is load-bearing: Drop order is the reverse.
    // sync must drop first (its Drop calls device_wait_idle); instance last.
    instance: Option<VulkanInstance>,
    device: Option<VulkanDevice>,
    swapchain: Option<VulkanSwapchain>,
    sync: Option<VulkanFrameSync>,
    ready: bool,
    fence_wait_required: bool,
}

impl VulkanRenderSurface {
    /// Initializes a Vulkan render surface from the configured native Metal
    /// layer.
    ///
    /// # Safety
    ///
    /// `config` must contain a non-null pointer to a live `CAMetalLayer`. The
    /// layer must remain valid and outlive the returned `VulkanRenderSurface`,
    /// and it must not be destroyed or replaced while the surface owns Vulkan
    /// objects created from it.
    /// # Errors
    ///
    /// Returns an error if Vulkan instance, device, swapchain, or
    /// synchronization setup fails.
    #[allow(clippy::needless_pass_by_value)]
    pub unsafe fn initialize(config: RenderSurfaceConfig) -> Result<Self, crate::EngineError> {
        let instance = unsafe { VulkanInstance::new(&config) }?;
        let device = unsafe { VulkanDevice::new(&instance) }?;
        let swapchain = unsafe { VulkanSwapchain::new(&instance, &device, &config) }?;
        let sync = unsafe { VulkanFrameSync::new(&device) }?;
        Ok(Self {
            instance: Some(instance),
            device: Some(device),
            swapchain: Some(swapchain),
            sync: Some(sync),
            ready: true,
            fence_wait_required: false,
        })
    }

    /// # Errors
    ///
    /// Returns an error if the surface is not ready or Vulkan command
    /// submission fails.
    ///
    /// # Panics
    ///
    /// Panics only if internal readiness bookkeeping says a Vulkan object is
    /// present while the corresponding object has been removed.
    #[allow(clippy::too_many_lines)]
    pub fn clear_and_present(&mut self, color: [f32; 4]) -> Result<(), crate::EngineError> {
        if !self.is_ready() {
            return Err(render_error("vulkan surface is not initialized"));
        }

        let device = self.device.as_ref().expect("device present while ready");
        let swapchain = self
            .swapchain
            .as_mut()
            .expect("swapchain present while ready");
        let sync = self.sync.as_ref().expect("sync present while ready");

        unsafe {
            if self.fence_wait_required {
                device
                    .device
                    .wait_for_fences(&[sync.in_flight_fence], true, u64::MAX)
                    .map_err(|error| vk_error("vkWaitForFences", error))?;
                self.fence_wait_required = false;
            }

            let (image_index, _) = device
                .swapchain_loader
                .acquire_next_image(
                    swapchain.swapchain,
                    u64::MAX,
                    sync.image_available_semaphore,
                    vk::Fence::null(),
                )
                .map_err(|error| vk_error("vkAcquireNextImageKHR", error))?;
            let image_index_usize = image_index as usize;
            let image = match swapchain.images.get(image_index_usize) {
                Some(image) => *image,
                None => {
                    return Err(self.post_acquire_error(render_error(
                        "swapchain image index was out of bounds",
                    )));
                }
            };

            if let Err(error) = device
                .device
                .reset_command_buffer(sync.command_buffer, vk::CommandBufferResetFlags::empty())
            {
                return Err(self.post_acquire_error(vk_error("vkResetCommandBuffer", error)));
            }

            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            if let Err(error) = device
                .device
                .begin_command_buffer(sync.command_buffer, &begin_info)
            {
                return Err(self.post_acquire_error(vk_error("vkBeginCommandBuffer", error)));
            }

            let color_range = vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1);
            let old_layout = if swapchain.image_initialized[image_index_usize] {
                vk::ImageLayout::PRESENT_SRC_KHR
            } else {
                vk::ImageLayout::UNDEFINED
            };
            let to_transfer = vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .old_layout(old_layout)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(color_range);
            device.device.cmd_pipeline_barrier(
                sync.command_buffer,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_transfer],
            );

            let clear_color = vk::ClearColorValue { float32: color };
            device.device.cmd_clear_color_image(
                sync.command_buffer,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &clear_color,
                &[color_range],
            );

            let to_present = vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::empty())
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(color_range);
            device.device.cmd_pipeline_barrier(
                sync.command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_present],
            );

            if let Err(error) = device.device.end_command_buffer(sync.command_buffer) {
                return Err(self.post_acquire_error(vk_error("vkEndCommandBuffer", error)));
            }

            let wait_semaphores = [sync.image_available_semaphore];
            let wait_stages = [vk::PipelineStageFlags::TRANSFER];
            let command_buffers = [sync.command_buffer];
            let signal_semaphores = [sync.render_finished_semaphore];
            let submit_info = vk::SubmitInfo::default()
                .wait_semaphores(&wait_semaphores)
                .wait_dst_stage_mask(&wait_stages)
                .command_buffers(&command_buffers)
                .signal_semaphores(&signal_semaphores);
            if let Err(error) = device.device.reset_fences(&[sync.in_flight_fence]) {
                return Err(self.post_acquire_error(vk_error("vkResetFences", error)));
            }
            if let Err(error) = device.device.queue_submit(
                device.graphics_queue,
                &[submit_info],
                sync.in_flight_fence,
            ) {
                return Err(self.post_acquire_error(vk_error("vkQueueSubmit", error)));
            }
            self.fence_wait_required = true;

            let swapchains = [swapchain.swapchain];
            let image_indices = [image_index];
            let present_info = vk::PresentInfoKHR::default()
                .wait_semaphores(&signal_semaphores)
                .swapchains(&swapchains)
                .image_indices(&image_indices);
            if let Err(error) = device
                .swapchain_loader
                .queue_present(device.present_queue, &present_info)
            {
                return Err(self.post_acquire_error(vk_error("vkQueuePresentKHR", error)));
            }
            swapchain.image_initialized[image_index_usize] = true;
        }

        Ok(())
    }

    fn post_acquire_error(&mut self, error: crate::EngineError) -> crate::EngineError {
        self.ready = false;
        self.fence_wait_required = false;
        error
    }

    pub fn shutdown(&mut self) {
        self.sync = None;
        self.swapchain = None;
        self.device = None;
        self.instance = None;
        self.ready = false;
        self.fence_wait_required = false;
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.ready
            && self.instance.is_some()
            && self.device.is_some()
            && self.swapchain.is_some()
            && self.sync.is_some()
    }
}

fn require_extension(
    extensions: &[vk::ExtensionProperties],
    name: &CStr,
    missing_message: &str,
) -> Result<(), crate::EngineError> {
    if has_extension(extensions, name) {
        Ok(())
    } else {
        Err(render_error(missing_message))
    }
}

fn has_extension(extensions: &[vk::ExtensionProperties], name: &CStr) -> bool {
    extensions.iter().any(|extension| {
        extension
            .extension_name_as_c_str()
            .is_ok_and(|extension_name| extension_name == name)
    })
}

fn invalid_input(message: impl Into<String>) -> crate::EngineError {
    crate::EngineError::InvalidInput(message.into())
}

fn render_error(message: impl Into<String>) -> crate::EngineError {
    crate::EngineError::Render(message.into())
}

fn vk_error(operation: &str, error: vk::Result) -> crate::EngineError {
    render_error(format!("{operation} failed with {error:?}"))
}
