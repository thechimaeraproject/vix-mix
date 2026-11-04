//! The window's surface and the egui renderer: the only wgpu the app owns. Vix's
//! `app/src/gpu.rs` without the preview textures.
//!
//! **Colour.** The surface is a non-sRGB format, which egui-wgpu prefers: it writes
//! gamma-encoded values straight into the framebuffer, so the clear colour is the theme's
//! window colour as written, in sRGB units.

use std::sync::Arc;

use egui_wgpu::ScreenDescriptor;
use winit::window::Window;

/// What the frame loop needs from wgpu.
pub struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    egui: egui_wgpu::Renderer,
    clear: wgpu::Color,
}

impl Gpu {
    /// A surface on `window`, an adapter that can present to it, a device, and an egui
    /// renderer; frames are cleared to `clear` (sRGB bytes) before egui draws.
    pub fn new(window: Arc<Window>, clear: egui::Color32) -> Result<Gpu, String> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(
                Box::new(Arc::clone(&window)),
            ));
        let surface = instance
            .create_surface(Arc::clone(&window))
            .map_err(|e| format!("no surface on the window: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU adapter can present to the window: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("vix-mix"),
            ..Default::default()
        }))
        .map_err(|e| format!("the adapter refused a device: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        let format = egui_wgpu::preferred_framebuffer_format(&caps.formats)
            .map_err(|e| format!("no framebuffer format egui can draw into: {e}"))?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| "the adapter cannot present to this surface".to_string())?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);

        let egui = egui_wgpu::Renderer::new(
            &device,
            format,
            egui_wgpu::RendererOptions {
                msaa_samples: 1,
                ..Default::default()
            },
        );
        let info = adapter.get_info();
        log::info!(
            "adapter: {} ({:?}); surface {:?} at {}×{}",
            info.name,
            info.backend,
            format,
            config.width,
            config.height
        );
        let [r, g, b, _] = clear.to_array();
        Ok(Gpu {
            surface,
            device,
            queue,
            config,
            egui,
            clear: wgpu::Color {
                r: f64::from(r) / 255.0,
                g: f64::from(g) / 255.0,
                b: f64::from(b) / 255.0,
                a: 1.0,
            },
        })
    }

    /// The largest texture side the device takes, for egui's font atlas.
    pub fn max_texture_side(&self) -> usize {
        self.device.limits().max_texture_dimension_2d as usize
    }

    /// Reconfigures the surface for a new window size. A zero size (minimised) is kept at one
    /// pixel, because a surface cannot be zero.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    /// Draws one egui frame into the next surface texture and presents it. Returns whether a
    /// frame reached the screen; a lost or outdated surface is reconfigured and the frame
    /// skipped, and the caller redraws.
    ///
    /// The texture deltas are applied whether or not a frame is drawn: egui hands each one
    /// out once, and a delta dropped unapplied is a font atlas the next frame lacks.
    pub fn render(
        &mut self,
        egui_ctx: &egui::Context,
        shapes: Vec<egui::epaint::ClippedShape>,
        mut textures_delta: egui::TexturesDelta,
        pixels_per_point: f32,
    ) -> Result<bool, String> {
        for (id, deltas) in &textures_delta.set {
            for delta in deltas {
                self.egui
                    .update_texture(&self.device, &self.queue, *id, delta);
            }
        }
        let free: Vec<egui::TextureId> = textures_delta.free.iter().copied().collect();
        textures_delta.clear();
        let free_all = |egui: &mut egui_wgpu::Renderer| {
            for id in &free {
                egui.free_texture(id);
            }
        };

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                free_all(&mut self.egui);
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                free_all(&mut self.egui);
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                free_all(&mut self.egui);
                return Err(
                    "the surface refused a frame (validation); run with RUST_LOG=wgpu=warn to see why"
                        .to_string(),
                );
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let screen = ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point,
        };
        let paint_jobs = egui_ctx.tessellate(shapes, pixels_per_point);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("vix-mix ui"),
            });
        let user_buffers = self.egui.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen,
        );
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("vix-mix ui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            let mut pass = pass.forget_lifetime();
            self.egui.render(&mut pass, &paint_jobs, &screen);
        }
        free_all(&mut self.egui);
        self.queue
            .submit(user_buffers.into_iter().chain([encoder.finish()]));
        self.queue.present(frame);
        Ok(true)
    }
}
