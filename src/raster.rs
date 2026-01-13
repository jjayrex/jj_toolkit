use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use rayon::prelude::*;
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Png,
    Bmp,
}

#[derive(Args)]
#[command[name = "rasterize", about = "Rasterize SVG images to PNG or BMP"]]
pub struct RasterizeArgs {
    input: PathBuf,
    /// Output input
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Output format
    #[arg(short, long, value_enum, default_value_t = OutputFormat::Png)]
    format: OutputFormat,
    /// Override output width in pixels
    #[arg(long)]
    width: Option<u32>,
    /// Override output height in pixels
    #[arg(long)]
    height: Option<u32>,
    /// Scale factor (applied after width/height)
    #[arg(short, long, default_value_t = 1.0)]
    scale: f32,
    /// Render recursively
    #[arg(short, long)]
    recursive: bool,
    /// Number of worker threads for batch mode (0 = use rayon default)
    #[arg(long, default_value_t = 0)]
    threads: usize,
    /// Overwrite existing files
    #[arg(long, default_value_t = false)]
    overwrite: bool,
}

pub fn rasterize(a: RasterizeArgs) -> Result<()> {
    if a.threads > 0 {
        rayon::ThreadPoolBuilder::new()
            .num_threads(a.threads)
            .build_global()
            .ok();
    }

    let input_meta = fs::metadata(&a.input)
        .with_context(|| format!("Failed to read input metadata: {}", a.input.display()))?;

    if input_meta.is_file() {
        rasterize_single(&a.input, a.output.as_deref(), &a)?;
    } else if input_meta.is_dir() {
        rasterize_batch(&a.input, a.output.as_deref(), &a)?;
    } else {
        bail!(
            "Input is neither a file nor a directory: {}",
            a.input.display()
        );
    }

    Ok(())
}

fn rasterize_single(input: &Path, output: Option<&Path>, a: &RasterizeArgs) -> Result<()> {
    ensure_svg(input)?;

    let output_path = resolve_output(input, output, a.format)?;
    if output_path.exists() && !a.overwrite {
        bail!("Output exists (use --overwrite: {}", output_path.display());
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("Create dir: {}", parent.display()))?;
    }

    render_svg(input, &output_path, a)?;
    Ok(())
}

fn rasterize_batch(input: &Path, output: Option<&Path>, a: &RasterizeArgs) -> Result<()> {
    let output_dir = match output {
        Some(path) => path.to_path_buf(),
        None => input.join("rasterized"),
    };
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("Create dir: {}", output_dir.display()))?;

    let mut walker = WalkDir::new(input);
    if !a.recursive {
        walker = walker.max_depth(1);
    }

    let svgs: Vec<PathBuf> = walker
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.into_path())
        .filter(|p| p.is_file() && is_svg(p))
        .collect();

    svgs.par_iter().try_for_each(|svg_path| -> Result<()> {
        let relative_path = svg_path.strip_prefix(input).unwrap_or(svg_path.as_path());

        let output_path = output_dir
            .join(relative_path)
            .with_extension(match a.format {
                OutputFormat::Png => "png",
                OutputFormat::Bmp => "bmp",
            });

        if output_path.exists() && !a.overwrite {
            return Ok(());
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Create dir: {}", parent.display()))?;
        }

        render_svg(svg_path, &output_path, a)?;
        Ok(())
    })?;

    Ok(())
}

fn render_svg(input: &Path, output: &Path, a: &RasterizeArgs) -> Result<()> {
    let data = fs::read(input).with_context(|| format!("Read SVG: {}", input.display()))?;

    let mut options = usvg::Options::default();
    options.resources_dir = input.parent().map(|p| p.to_path_buf());

    std::sync::Arc::make_mut(&mut options.fontdb).load_system_fonts();

    let tree = usvg::Tree::from_data(&data, &options)
        .with_context(|| format!("Parse SVG: {}", input.display()))?;

    let size = tree.size();
    let mut width = size.width().ceil() as u32;
    let mut height = size.height().ceil() as u32;

    match (a.width, a.height) {
        (Some(w), Some(h)) => {
            width = w;
            height = h;
        }
        (Some(w), None) => {
            let aspect = (height as f32) / (width as f32);
            width = w;
            height = (w as f32 * aspect).round().max(1.0) as u32;
        }
        (None, Some(h)) => {
            let aspect = (width as f32) / (h as f32);
            height = h;
            width = (h as f32 * aspect).round().max(1.0) as u32;
        }
        (None, None) => {}
    }

    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .with_context(|| format!("Allocate Pixmap {}x{}", width, height))?;

    let source_width = size.width() as f32;
    let source_height = size.height() as f32;

    let target_width = width as f32;
    let target_height = height as f32;

    let scale_x = target_width / source_width;
    let scale_y = target_height / source_height;
    let scale = scale_x.min(scale_y);

    let transform_x = (target_width - source_width * scale) * 0.5;
    let transform_y = (target_height - source_height * scale) * 0.5;

    let transform = tiny_skia::Transform::from_scale(scale, scale).post_translate(transform_x, transform_y);

    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let rgba = pixmap.data().to_vec();
    let img = image::RgbaImage::from_raw(width, height, rgba)
        .with_context(|| "pixmap -> image buffer - conversion failed")?;

    match a.format {
        OutputFormat::Png => img
            .save_with_format(output, image::ImageFormat::Png)
            .with_context(|| format!("Write PNG: {}", output.display()))?,
        OutputFormat::Bmp => img
            .save_with_format(output, image::ImageFormat::Bmp)
            .with_context(|| format!("Write BMP: {}", output.display()))?,
    }

    Ok(())
}

fn resolve_output(input: &Path, output: Option<&Path>, format: OutputFormat) -> Result<PathBuf> {
    let extension = match format {
        OutputFormat::Png => "png",
        OutputFormat::Bmp => "bmp",
    };

    let default_output = input.with_extension(extension);

    let Some(out) = output else {
        return Ok(default_output);
    };

    if out.exists() && out.is_dir() {
        let file_stem = input
            .file_stem()
            .and_then(OsStr::to_str)
            .unwrap_or("output");
        return Ok(out.join(format!("{file_stem}.{extension}")));
    }

    if out.extension().is_none() {
        let file_stem = input
            .file_stem()
            .and_then(OsStr::to_str)
            .unwrap_or("output");
        return Ok(out.join(format!("{file_stem}.{extension}")));
    }

    Ok(out.to_path_buf())
}

fn ensure_svg(input: &Path) -> Result<()> {
    if !is_svg(input) {
        bail!("Not an .svg file: {}", input.display());
    }
    Ok(())
}

fn is_svg(input: &Path) -> bool {
    input
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("svg"))
        .unwrap_or(false)
}
