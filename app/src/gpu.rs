//! The window's surface and the egui renderer: the only wgpu the app owns. Vix's
//! `app/src/gpu.rs` without the preview textures. [`upload`] and [`encode`] are the one paint
//! path: the window draws through them into its surface, and `--dump-png`
//! ([`crate::capture`]) into a texture it reads back. Both make their device from
//! [`device_descriptor`], which asks for small memory blocks.
//!
//! **Colour.** The surface is a non-sRGB format, which egui-wgpu prefers: it writes
//! gamma-encoded values straight into the framebuffer, so the clear colour is the theme's
//! window colour as written, in sRGB units.

use std::sync::Arc;

use egui_wgpu::ScreenDescriptor;
use winit::window::Window;

/// What to do when the GPU or its driver refuses the window's drawing.
pub const DRIVER_FIX: &str = "update the GPU driver, or set WGPU_BACKEND to another backend this machine has (vulkan, dx12, metal or gl)";

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
            .map_err(|e| format!("no surface on the window ({e}); {DRIVER_FIX}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU adapter can present to the window ({e}); {DRIVER_FIX}"))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&device_descriptor("vix-mix")))
                .map_err(|e| format!("the adapter refused a device ({e}); {DRIVER_FIX}"))?;

        let caps = surface.get_capabilities(&adapter);
        let format = egui_wgpu::preferred_framebuffer_format(&caps.formats)
            .map_err(|e| format!("no framebuffer format egui can draw into ({e}); {DRIVER_FIX}"))?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| format!("the adapter cannot present to this surface; {DRIVER_FIX}"))?;
        config.format = format;
        // Mailbox where the surface offers it: the window paces itself to 60 Hz, and FIFO's
        // acquire can block for tens of milliseconds behind a busy compositor (measured on
        // Windows with overlay layers installed: 48 frames a second against mailbox's 60).
        config.present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo
        };
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
            "adapter: {} ({:?}); surface {:?} at {}×{}, {:?}",
            info.name,
            info.backend,
            format,
            config.width,
            config.height,
            config.present_mode
        );
        Ok(Gpu {
            surface,
            device,
            queue,
            config,
            egui,
            clear: clear_colour(clear),
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
        textures_delta: egui::TexturesDelta,
        pixels_per_point: f32,
    ) -> Result<bool, String> {
        let free = upload(&mut self.egui, &self.device, &self.queue, textures_delta);
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
        let user_buffers = encode(
            &mut self.egui,
            &self.device,
            &self.queue,
            &mut encoder,
            &view,
            self.clear,
            &paint_jobs,
            &screen,
        );
        free_all(&mut self.egui);
        self.queue
            .submit(user_buffers.into_iter().chain([encoder.finish()]));
        self.queue.present(frame);
        Ok(true)
    }
}

/// The device the window and the capture both ask for: wgpu's defaults, with small memory
/// blocks.
///
/// wgpu takes GPU memory from the driver in blocks and hands out pieces of them. Its default
/// hint is sized for games: a 64 MiB block for memory the CPU writes and a 128 MiB one for
/// memory only the card touches. On an NVIDIA card without Resizable BAR the first of those
/// comes out of a 256 MiB window that every program on the desktop shares (BAR1), and it stays
/// mapped for as long as the device lives, which for a mixer is the whole session. When that
/// window is full the driver refuses the next mapping, the compositor's included. A frame of
/// this console is a few hundred kilobytes, so the device asks for the small blocks instead:
/// 4 MiB and 8 MiB, the allocator adding more only when one fills.
pub fn device_descriptor(label: &'static str) -> wgpu::DeviceDescriptor<'static> {
    wgpu::DeviceDescriptor {
        label: Some(label),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        ..Default::default()
    }
}

/// `colour` (sRGB bytes) as the clear colour of a non-sRGB target: the bytes as written.
pub fn clear_colour(colour: egui::Color32) -> wgpu::Color {
    let [r, g, b, _] = colour.to_array();
    wgpu::Color {
        r: f64::from(r) / 255.0,
        g: f64::from(g) / 255.0,
        b: f64::from(b) / 255.0,
        a: 1.0,
    }
}

/// Hands egui's new and changed textures (the font atlas among them) to the renderer and
/// returns the ones to free once the frame is submitted.
///
/// The deltas are applied whether or not a frame is drawn: egui hands each one out once, and
/// a delta dropped unapplied is a font atlas the next frame lacks.
pub fn upload(
    egui: &mut egui_wgpu::Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mut textures_delta: egui::TexturesDelta,
) -> Vec<egui::TextureId> {
    for (id, deltas) in &textures_delta.set {
        for delta in deltas {
            egui.update_texture(device, queue, *id, delta);
        }
    }
    let free = textures_delta.free.iter().copied().collect();
    textures_delta.clear();
    free
}

/// Encodes one egui frame into `target`, cleared to `clear` first. Returns egui's own command
/// buffers, which are submitted before `encoder`'s.
#[allow(clippy::too_many_arguments)]
pub fn encode(
    egui: &mut egui_wgpu::Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    clear: wgpu::Color,
    paint_jobs: &[egui::ClippedPrimitive],
    screen: &ScreenDescriptor,
) -> Vec<wgpu::CommandBuffer> {
    let user_buffers = egui.update_buffers(device, queue, encoder, paint_jobs, screen);
    let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("vix-mix ui"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    let mut pass = pass.forget_lifetime();
    egui.render(&mut pass, paint_jobs, screen);
    user_buffers
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1024 * 1024;

    /// Under the smallest block wgpu's default hint takes (64 MiB, for memory the CPU
    /// writes), and over what the small blocks come to here (4 MiB and 8 MiB).
    const RESERVED_AT_MOST: u64 = 32 * MIB;

    #[test]
    fn a_frames_upload_reserves_megabytes_of_the_card_not_a_games_blocks() {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let Ok(adapter) =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        else {
            eprintln!("skipped: no GPU adapter on this machine, so there is no device to measure");
            return;
        };
        let info = adapter.get_info();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&device_descriptor("vix-mix test")))
                .expect("the adapter gives a device");

        // What a frame of egui does: bytes the CPU wrote, copied into a buffer on the card.
        let bytes = vec![0u8; 64 * 1024];
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vix-mix test vertices"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&vertices, 0, &bytes);
        queue.submit([]);

        let Some(report) = device.generate_allocator_report() else {
            eprintln!(
                "skipped: {} ({:?}) does not report what its allocator reserved",
                info.name, info.backend
            );
            return;
        };
        eprintln!(
            "{} ({:?}) reserved {} KiB in {} blocks",
            info.name,
            info.backend,
            report.total_reserved_bytes / 1024,
            report.blocks.len()
        );
        assert!(
            report.total_reserved_bytes <= RESERVED_AT_MOST,
            "one 64 KiB upload reserved {} MiB of the card on {} ({:?}); `device_descriptor` \
             asks for small blocks with `MemoryHints::MemoryUsage`, and without that hint the \
             first block alone is 64 MiB",
            report.total_reserved_bytes / MIB,
            info.name,
            info.backend
        );
    }
}
