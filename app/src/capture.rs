//! `--dump-png <file>`: the look gate's capture. One frame of the window — the header and the
//! console — drawn from `FakeHost`'s example console after [`WARM_UP_MS`] of its simulated
//! meters (so the peak-hold lines stand above the levels as they do in the prototype's
//! screenshot), at the prototype's scale ([`SCALE`] pixels a point), through the window's own
//! paint path ([`gpu::upload`], [`gpu::encode`]) into an offscreen texture, read back and
//! written as an 8-bit RGBA PNG. No window, no event loop, no display: it needs a GPU adapter
//! and nothing else. `FakeHost` is seeded, so a run draws the same frame every time on the
//! same adapter.

use std::path::Path;
use std::sync::mpsc;

use egui::{Pos2, Rect, ViewportId};
use egui_wgpu::ScreenDescriptor;
use vixmix_host::{BackendState, FakeHost, Host};

use crate::gpu;
use crate::meters::{FRAME_MS, Meters};
use crate::theme::colour;
use crate::ui;

/// Pixels a point: the prototype's screenshot is a 2× capture of its 902 × 490 window.
pub const SCALE: f32 = 2.0;
/// How long `FakeHost`'s meters run before the frame is drawn.
pub const WARM_UP_MS: f64 = 2_500.0;
/// The texture format the frame is drawn in: non-sRGB, as the window's surface, so the PNG
/// holds the bytes the window would show.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// What to do when the GPU fails the capture part-way: once may be a driver hiccup.
const GPU_FIX: &str = "run it again, and if it fails again update the GPU driver or set WGPU_BACKEND to another backend this machine has (vulkan, dx12, metal or gl)";
/// What to do when the PNG cannot be written once the file is open.
const DISK_FIX: &str =
    "check the disk has room and nothing else holds the file open, then run it again";

/// Draws the frame and writes it to `path`; returns its size in pixels.
pub fn dump_png(path: &Path) -> Result<[u32; 2], String> {
    let mut host = FakeHost::new();
    let mut meters = Meters::default();
    let frames = (WARM_UP_MS / FRAME_MS).round() as u32;
    for frame in 1..=frames {
        host.advance(FRAME_MS);
        meters.update(&host.meters(), f64::from(frame) * FRAME_MS);
    }
    let view = ui::View {
        backend: &BackendState::Simulated,
        stats: host.stats(),
        setup: host.setup(),
        meters: &meters,
        apps: host.apps(),
    };

    let ctx = egui::Context::default();
    ui::install(&ctx);
    let points = ui::natural_size(host.setup());
    let mut input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, points)),
        ..Default::default()
    };
    input
        .viewports
        .entry(ViewportId::ROOT)
        .or_default()
        .native_pixels_per_point = Some(SCALE);
    // Two passes, as egui lays out panels from the previous pass's sizes; the font atlas
    // both passes built goes to the renderer.
    let mut textures = egui::TexturesDelta::default();
    let mut output = None;
    for _ in 0..2 {
        let mut pass = ctx.run_ui(input.clone(), |root| ui::draw(root, &view));
        textures.append(std::mem::take(&mut pass.textures_delta));
        output = Some(pass);
    }
    let output = output.expect("two passes ran");
    let pixels_per_point = output.pixels_per_point;
    let width = (points.x * pixels_per_point).round() as u32;
    let height = (points.y * pixels_per_point).round() as u32;
    let paint_jobs = ctx.tessellate(output.shapes, pixels_per_point);

    let rgba = render(width, height, pixels_per_point, textures, &paint_jobs)?;
    write_png(path, width, height, &rgba)?;
    Ok([width, height])
}

/// Paints `paint_jobs` into a `width` × `height` texture and reads it back as tightly packed
/// RGBA rows.
fn render(
    width: u32,
    height: u32,
    pixels_per_point: f32,
    textures: egui::TexturesDelta,
    paint_jobs: &[egui::ClippedPrimitive],
) -> Result<Vec<u8>, String> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        ..Default::default()
    }))
    .map_err(|e| {
        format!("no GPU adapter to draw the frame on ({e}); run it where a GPU driver is installed, or set WGPU_BACKEND to one this machine has")
    })?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("vix-mix capture"),
        ..Default::default()
    }))
    .map_err(|e| format!("the adapter refused a device ({e}); {}", gpu::DRIVER_FIX))?;
    let info = adapter.get_info();
    log::info!("capture adapter: {} ({:?})", info.name, info.backend);

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("vix-mix capture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let padded = padded_row(width);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vix-mix capture readback"),
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = egui_wgpu::Renderer::new(
        &device,
        FORMAT,
        egui_wgpu::RendererOptions {
            msaa_samples: 1,
            ..Default::default()
        },
    );
    let free = gpu::upload(&mut renderer, &device, &queue, textures);
    let screen = ScreenDescriptor {
        size_in_pixels: [width, height],
        pixels_per_point,
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("vix-mix capture"),
    });
    let user_buffers = gpu::encode(
        &mut renderer,
        &device,
        &queue,
        &mut encoder,
        &view,
        gpu::clear_colour(colour::WIN),
        paint_jobs,
        &screen,
    );
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(user_buffers.into_iter().chain([encoder.finish()]));
    for id in &free {
        renderer.free_texture(id);
    }

    let (sender, receiver) = mpsc::sync_channel(1);
    buffer.map_async(wgpu::MapMode::Read, .., move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| format!("the GPU did not finish the frame ({e}); {GPU_FIX}"))?;
    receiver
        .recv()
        .map_err(|_| format!("the GPU dropped the read-back before answering; {GPU_FIX}"))?
        .map_err(|e| format!("the frame could not be read back from the GPU ({e}); {GPU_FIX}"))?;
    let mapped = buffer
        .get_mapped_range(..)
        .map_err(|e| format!("the read-back buffer is not readable ({e}); {GPU_FIX}"))?;
    Ok(unpad(&mapped, width, height, padded))
}

/// A row's bytes rounded up to what a texture-to-buffer copy wants (256-byte rows).
fn padded_row(width: u32) -> u32 {
    let row = width * 4;
    row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
}

/// Drops each row's padding: `height` rows of `width` RGBA pixels, `padded` bytes apart.
fn unpad(bytes: &[u8], width: u32, height: u32, padded: u32) -> Vec<u8> {
    let row = width as usize * 4;
    bytes
        .chunks(padded as usize)
        .take(height as usize)
        .flat_map(|line| &line[..row])
        .copied()
        .collect()
}

/// The pixels as an 8-bit RGBA PNG (Vix's `write_png`).
fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| {
        format!(
            "cannot write {} ({e}); check the directory exists and is writable",
            path.display()
        )
    })?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| {
        format!(
            "cannot write the PNG header of {} ({e}); {DISK_FIX}",
            path.display()
        )
    })?;
    writer.write_image_data(rgba).map_err(|e| {
        format!(
            "cannot write the pixels of {} ({e}); {DISK_FIX}",
            path.display()
        )
    })?;
    writer
        .finish()
        .map_err(|e| format!("cannot finish {} ({e}); {DISK_FIX}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_back_rows_lose_their_padding() {
        // 3 pixels wide: 12 bytes of each 256-byte row are the image.
        let padded = padded_row(3);
        assert_eq!(padded, 256);
        assert_eq!(padded_row(64), 256);
        assert_eq!(padded_row(65), 512);
        let mut bytes = vec![0u8; padded as usize * 2];
        bytes[..12].copy_from_slice(&[1; 12]);
        bytes[256..268].copy_from_slice(&[2; 12]);
        let rgba = unpad(&bytes, 3, 2, padded);
        assert_eq!(rgba.len(), 24);
        assert!(rgba[..12].iter().all(|&b| b == 1));
        assert!(rgba[12..].iter().all(|&b| b == 2));
    }

    #[test]
    fn the_png_holds_the_pixels_as_drawn() {
        let dir = std::env::temp_dir().join(format!("vix-mix-capture-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("frame.png");
        let rgba: Vec<u8> = (0..2 * 3 * 4).map(|i| i as u8 * 10).collect();
        write_png(&path, 2, 3, &rgba).unwrap();
        let decoder =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut back = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut back).unwrap();
        assert_eq!((info.width, info.height), (2, 3));
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(&back[..info.buffer_size()], &rgba[..]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unwritable_path_says_what_to_check() {
        let error = write_png(Path::new("/no/such/dir/frame.png"), 1, 1, &[0; 4]).unwrap_err();
        assert!(error.contains("check the directory exists"), "{error}");
    }
}
