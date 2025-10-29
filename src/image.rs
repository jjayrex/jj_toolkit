use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use std::fs::File;
use std::path::{Path, PathBuf};
use image::ImageEncoder;

#[derive(Subcommand)]
pub enum ImageCmd {
    Convert(ConvertArgs),
}

#[derive(Clone, Copy, ValueEnum, Debug)]
pub enum ImageFormat { Png, Jpeg, Webp, Bmp, Ico, Tiff, Tga, Dds, Pnm }

#[derive(Args)]
pub struct ConvertArgs {
    pub input: PathBuf,
    #[arg(short, long, value_enum)]
    pub format: ImageFormat,
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    // Quality for JPEG. 1-100. Default: 90
    #[arg(long, default_value_t = 90)]
    pub quality: u8,
    // Background color for formats without Alpha. Default: FFFFFF
    #[arg(long, default_value = "FFFFFF")]
    pub background: String,
}

pub fn run(cmd: ImageCmd) -> Result<()> {
    match cmd {
        ImageCmd::Convert(a) => convert_cmd(a),
    }
}

fn convert_cmd(a: ConvertArgs) -> Result<()> {
    let image = image::open(&a.input)
        .with_context(|| format!("open {}", a.input.display()))?;
    let output = a.output.unwrap_or_else(|| {
        let stem = a.input.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "output".to_string());
        PathBuf::from(format!("{}.{}", stem, ext_for(a.format)))
    });

    match a.format {
        ImageFormat::Png => save_png(&image, &output)?,
        ImageFormat::Bmp => save_common(&image, &output, ImageFormat::Bmp)?,
        ImageFormat::Ico => save_common(&image, &output, ImageFormat::Ico)?,
        ImageFormat::Tiff => save_common(&image, &output, ImageFormat::Tiff)?,
        ImageFormat::Tga => save_common(&image, &output, ImageFormat::Tga)?,
        ImageFormat::Dds => save_common(&image, &output, ImageFormat::Dds)?,
        ImageFormat::Pnm => save_common(&image, &output, ImageFormat::Pnm)?,
        ImageFormat::Jpeg => {
            let bg = parse_hex_rgb(&a.background)?;
            save_jpeg(&image, &output, a.quality, bg)?
        }
        ImageFormat::Webp => save_webp(&image, &output)?,
    }

    println!("Wrote {}", output.display());
    Ok(())
}

// ENCODERS
fn save_png(image: &image::DynamicImage, output: &Path) -> Result<()> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    let f = File::create(output)?;
    let enc = PngEncoder::new_with_quality(f, CompressionType::Default, FilterType::Adaptive);
    let rgba = image.to_rgba8();
    enc.write_image(&rgba, rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)?;
    Ok(())
}

fn save_jpeg(image: &image::DynamicImage, output: &Path, quality: u8, bg: (u8,u8,u8)) -> Result<()> {
    use image::codecs::jpeg::JpegEncoder;
    let f = File::create(output)?;
    let mut enc = JpegEncoder::new_with_quality(f, quality.clamp(1, 100));
    let rgb = flatten_to_rgb8(image, bg);
    enc.encode(&rgb, rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)?;
    Ok(())
}

fn save_webp(image: &image::DynamicImage, output: &Path) -> Result<()> {
    use image::codecs::webp::WebPEncoder;
    let f = File::create(output)?;
    let rgba = image.to_rgba8();
    let enc = WebPEncoder::new_lossless(f);
    enc.encode(&rgba, rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)?;
    Ok(())
}

fn save_common(image: &image::DynamicImage, output: &Path, format: ImageFormat) -> Result<()> {
    match format {
        ImageFormat::Bmp => image.save_with_format(output, image::ImageFormat::Bmp)?,
        ImageFormat::Ico => image.save_with_format(output, image::ImageFormat::Ico)?,
        ImageFormat::Tiff => image.save_with_format(output, image::ImageFormat::Tiff)?,
        ImageFormat::Tga => image.save_with_format(output, image::ImageFormat::Tga)?,
        ImageFormat::Dds => image.save_with_format(output, image::ImageFormat::Dds)?,
        ImageFormat::Pnm => image.save_with_format(output, image::ImageFormat::Pnm)?,
        _ => panic!("unsupported image format"),
    }
    Ok(())
}

// HELPERS
fn ext_for(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Webp => "webp",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Ico => "ico",
        ImageFormat::Tiff => "tiff",
        ImageFormat::Tga => "tga",
        ImageFormat::Dds => "dds",
        ImageFormat::Pnm => "pnm",
    }
}

fn parse_hex_rgb(s: &str) -> Result<(u8,u8,u8)> {
    let t = s.trim().trim_start_matches('#');
    let err = || anyhow::anyhow!("invalid hex color '{}'", s);
    if t.len() == 6 {
        let r = u8::from_str_radix(&t[0..2], 16).map_err(|_| err())?;
        let g = u8::from_str_radix(&t[2..4], 16).map_err(|_| err())?;
        let b = u8::from_str_radix(&t[4..6], 16).map_err(|_| err())?;
        Ok((r,g,b))
    } else {
        bail!(err());
    }
}

fn flatten_to_rgb8(image: &image::DynamicImage, bg: (u8,u8,u8)) -> image::ImageBuffer<image::Rgb<u8>, Vec<u8>> {
    use image::{GenericImageView, Rgba};
    let (w, h) = image.dimensions();
    let mut output = image::ImageBuffer::new(w, h);
    let rgba = image.to_rgba8();
    for (x, y, p) in rgba.enumerate_pixels() {
        let Rgba([r, g, b, a]) = *p;
        let (br, bgc, bb) = bg;
        let a_f = (a as f32) / 255.0;
        let inv = 1.0 - a_f;
        let nr = (r as f32 * a_f + br as f32 * inv).round() as u8;
        let ng = (g as f32 * a_f + bgc as f32 * inv).round() as u8;
        let nb = (b as f32 * a_f + bb as f32 * inv).round() as u8;
        output.put_pixel(x, y, image::Rgb([nr, ng, nb]));
    }
    output
}