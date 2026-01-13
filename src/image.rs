use anyhow::{bail, Context, Result};
use clap::{Args, ValueEnum};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::io::{BufWriter, Write};
use std::collections::BTreeSet;
use image::{GenericImageView, ImageEncoder};

#[derive(Clone, Copy, ValueEnum, Debug)]
pub enum ImageFormat { Png, Jpeg, Webp, Bmp, Ico, Tiff, Tga, Dds, Pnm }

#[derive(Clone, Copy, ValueEnum, Debug)]
pub enum ResizeMode { Fit, Fill, Exact }

#[derive(Clone, Copy, ValueEnum, Debug)]
pub enum Filter { Nearest, Triangle, CatmullRom, Gaussian, Lanczos3 }

#[derive(Args)]
#[command[name = "image-convert", about = "Simple image format conversion supporting PNG, JPEG, WEBP, BMP, ICO, TIFF, TGA, DDS and PNM"]]
pub struct ConvertArgs {
    input: PathBuf,
    #[arg(short, long, value_enum)]
    format: ImageFormat,
    #[arg(short, long)]
    output: Option<PathBuf>,
    // Quality for JPEG. 1-100. Default: 90
    #[arg(long, default_value_t = 90)]
    quality: u8,
    // Background color for formats without Alpha. Default: FFFFFF
    #[arg(long, default_value = "FFFFFF")]
    background: String,
}

#[derive(Args)]
#[command[name = "image-scale", about = "Simple image scaling using Lanczos3, Nearest, Triangle, Catmullrom or Gaussian resampling"]]
pub struct ScaleArgs {
    input: PathBuf,
    #[arg(short, long)]
    percent: Option<u32>,
    #[arg(long)]
    width: Option<u32>,
    #[arg(long)]
    height: Option<u32>,
    // fit | fill | exact
    #[arg(long, value_enum, default_value_t = ResizeMode::Fit)]
    mode: ResizeMode,
    // Resampling filter
    #[arg(long, value_enum, default_value_t = Filter::Lanczos3)]
    filter: Filter,
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(Args)]
#[command[name = "image-getcolor", about = "Get all colors present in an image"]]
pub struct GetColorArgs {
    input: PathBuf,
    #[arg(short, long)]
    output: Option<PathBuf>,
}

pub fn convert(a: ConvertArgs) -> Result<()> {
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

pub fn scale(a: ScaleArgs) -> Result<()> {
    use image::imageops::resize;
    let image = image::open(&a.input).with_context(|| format!("open {}", a.input.display()))?;
    let (w, h) = image.dimensions();

    // Determine target size
    let (tw, th) = compute_target_size(w, h, a.percent, a.width, a.height)?;
    let f = filter_to_type(a.filter);

    let output_image = match a.mode {
        ResizeMode::Exact => resize(&image, tw, th, f),
        ResizeMode::Fit => {
            resize(&image, tw, th, f)
        }
        ResizeMode::Fill => {
            // scale to cover and then center-crop
            let (cw, ch) = cover_size(w, h, tw, th);
            let tmp = resize(&image, cw, ch, f);
            let x = (cw.saturating_sub(tw)) / 2;
            let y = (ch.saturating_sub(th)) / 2;
            image::imageops::crop_imm(&tmp, x, y, tw, th).to_image()
        }
    };

    let output = a.output.unwrap_or_else(|| {
        let stem = a.input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into());
        let ext = a.input.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "png".into());
        PathBuf::from(format!("{}_{}x{}.{}", stem, tw, th, ext))
    });

    output_image.save(&output)?;
    println!("Wrote {}", output.display());
    Ok(())
}

pub fn get_color(a: GetColorArgs) -> Result<()> {
    let image = image::open(&a.input).with_context(|| format!("failed to open image: {}", a.input.display()))?;

    let rgba = image.to_rgba8();
    let mut unique_colors = BTreeSet::<u32>::new();

    for pixel in rgba.pixels() {
        let [r, g, b, a] = pixel.0;
        let packed = u32::from_be_bytes([r, g, b, a]);
        unique_colors.insert(packed);
    }

    match a.output {
        Some(path) => {
            let file = File::create(&path).with_context(|| {
                format!("failed to create output file: {}", path.to_string_lossy())
            })?;
            let mut writer = BufWriter::new(file);
            write_colors(&mut writer, &unique_colors)?;
        }
        None => {
            let mut stdout = BufWriter::new(std::io::stdout());
            write_colors(&mut stdout, &unique_colors)?;
        }
    }

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

fn filter_to_type(f: Filter) -> image::imageops::FilterType {
    use image::imageops::FilterType;
    match f {
        Filter::Nearest => FilterType::Nearest,
        Filter::Triangle => FilterType::Triangle,
        Filter::CatmullRom => FilterType::CatmullRom,
        Filter::Gaussian => FilterType::Gaussian,
        Filter::Lanczos3 => FilterType::Lanczos3,
    }
}

fn compute_target_size(
    w: u32, h: u32,
    percent: Option<u32>, width: Option<u32>, height: Option<u32>
) -> Result<(u32, u32)> {
    if percent.is_none() && width.is_none() && height.is_none() {
        bail!("provide --percent or --width/--height");
    }
    if let Some(p) = percent {
        if width.is_none() && height.is_none() {
            let s = (p as f32) / 100.0;
            return Ok(((w as f32 * s).round().max(1.0) as u32,
                       (h as f32 * s).round().max(1.0) as u32));
        }
    }
    match (width, height) {
        (Some(tw), Some(th)) => Ok((tw, th)),
        (Some(tw), None) => {
            let th = ((tw as f32) * (h as f32) / (w as f32)).round().max(1.0) as u32;
            Ok((tw, th))
        }
        (None, Some(th)) => {
            let tw = ((th as f32) * (w as f32) / (h as f32)).round().max(1.0) as u32;
            Ok((tw, th))
        }
        (None, None) => unreachable!(),
    }
}

fn cover_size(w: u32, h: u32, tw: u32, th: u32) -> (u32, u32) {
    let sr = w as f32 / h as f32;
    let tr = tw as f32 / th as f32;
    if sr > tr {
        // source wider: scale by height
        let scale = th as f32 / h as f32;
        ((w as f32 * scale).round() as u32, th)
    } else {
        // source taller: scale by width
        let scale = tw as f32 / w as f32;
        (tw, (h as f32 * scale).round() as u32)
    }
}

fn write_colors<W: Write>(mut writer: W, colors: &BTreeSet<u32>) -> Result<()> {
    for packed in colors {
        let [r, g, b, a] = packed.to_be_bytes();
        // Hex format: #RRGGBBAA
        writeln!(&mut writer, "#{:02X}{:02X}{:02X}{:02X}", r, g, b, a)?;
    }
    Ok(())
}
