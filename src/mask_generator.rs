use image::{GenericImage, Pixel, Primitive, Rgb, Rgba};
use num_traits::cast::ToPrimitive;
use num_traits::NumCast;
use serde::Deserialize;
use tiny_skia::IntSize;
use xml_builder::{XMLBuilder, XMLElement, XMLVersion};

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type")]
pub enum MaskContent {
    Stripes {
        color1: Color,
        color2: Color,
        stripe_count: u32,
    },
    Text {
        text: String,
        font: String,
        color: Color,
        size_percent: f32,
        rotation: f32,
        row_slide_percent: f32,
        offset_x_percent: f32,
        stride_x_percent: f32,
        offset_y_percent: f32,
        stride_y_percent: f32,
    },
}

#[derive(Clone, Debug, Deserialize)]
pub struct MaskConfig {
    alpha: u8,
    content: MaskContent,
}

#[derive(Clone, Debug)]
pub struct Color {
    rgb: [u8; 3],
}

impl Color {
    pub fn hex_with_alpha(&self, alpha: u8) -> String {
        let [r, g, b] = self.rgb;
        format!("#{:02x}{:02x}{:02x}{:02x}", r, g, b, alpha)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let color = String::deserialize(deserializer)?;

        let color = color.trim_start_matches('#');

        // check length
        if color.len() != 6 {
            return Err(serde::de::Error::custom("color must be 8 characters long"));
        }
        // check if it's a valid hex string
        if !color.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(serde::de::Error::custom("color must be a valid hex string"));
        }

        let r = u8::from_str_radix(&color[0..2], 16).map_err(serde::de::Error::custom)?;
        let g = u8::from_str_radix(&color[2..4], 16).map_err(serde::de::Error::custom)?;
        let b = u8::from_str_radix(&color[4..6], 16).map_err(serde::de::Error::custom)?;

        Ok(Self { rgb: [r, g, b] })
    }
}

struct CoordinateIter {
    position: i32,
    stride: i32,
    size: i32,
}

impl CoordinateIter {
    pub fn new(position: i32, stride: u32, size: u32) -> Self {
        Self {
            position,
            stride: stride.try_into().unwrap(),
            size: size.try_into().unwrap(),
        }
    }
}

impl Iterator for CoordinateIter {
    type Item = i32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.position > self.size {
            return None;
        }

        let result = self.position;
        self.position += self.stride;

        Some(result)
    }
}

fn generate_mask_svg(mask: MaskConfig, mask_width: u32, mask_height: u32) -> String {
    let mut xml = XMLBuilder::new()
        .version(XMLVersion::XML1_1)
        .encoding("UTF-8".into())
        .build();

    let mut svg = XMLElement::new("svg");
    svg.add_attribute("xmlns", "http://www.w3.org/2000/svg");
    svg.add_attribute("viewbox", &format!("0 0 {mask_width} {mask_height}"));

    let MaskConfig {
        alpha,
        content: mask_content,
    } = mask;

    match mask_content {
        MaskContent::Stripes {
            color1,
            color2,
            stripe_count,
        } => {
            let color1 = color1.hex_with_alpha(alpha);
            let color2 = color2.hex_with_alpha(alpha);

            for i in 0..stripe_count {
                let y = mask_height * i / stripe_count;
                let height = mask_height / stripe_count;
                let color = if i % 2 == 0 { &color1 } else { &color2 };

                let mut rect = XMLElement::new("rect");
                rect.add_attribute("x", "0");
                rect.add_attribute("y", &y.to_string());
                rect.add_attribute("width", &mask_width.to_string());
                rect.add_attribute("height", &height.to_string());
                rect.add_attribute("fill", color);

                svg.add_child(rect).unwrap();
            }
        }
        MaskContent::Text {
            text: mask_text,
            font,
            color,
            size_percent,
            rotation,
            row_slide_percent,
            offset_x_percent,
            stride_x_percent,
            offset_y_percent,
            stride_y_percent,
        } => {
            let color = color.hex_with_alpha(alpha);

            let base_size = std::cmp::max(mask_width, mask_height) as f32 * 0.01;
            let font_size = base_size * size_percent;
            let font_size = format!("{font_size:.2}");

            let row_slide = (base_size * row_slide_percent) as i32;
            let offset_x = (base_size * offset_x_percent) as i32;
            let stride_x = (base_size * stride_x_percent) as u32;
            let offset_y = (base_size * offset_y_percent) as i32;
            let stride_y = (base_size * stride_y_percent) as u32;

            let mut slide = 0;
            for y in CoordinateIter::new(offset_y, stride_y, mask_height) {
                for x in CoordinateIter::new(slide + offset_x, stride_x, mask_width) {
                    let mut text = XMLElement::new("text");
                    text.add_attribute("x", &x.to_string());
                    text.add_attribute("y", &y.to_string());
                    text.add_attribute("fill", &color);
                    text.add_attribute("font-family", &font);
                    text.add_attribute("font-size", &font_size);
                    text.add_attribute("transform", &format!("rotate({} {} {})", rotation, x, y));
                    text.add_text(mask_text.clone()).unwrap();

                    svg.add_child(text).unwrap();
                }
                slide += row_slide;
            }
        }
    }

    xml.set_root_element(svg);

    let mut svg = Vec::<u8>::new();

    xml.generate(&mut svg).unwrap();

    let svg = svg
        .strip_prefix(br#"<?xml version="1.1" encoding="UTF-8"?>"#)
        .unwrap();
    std::str::from_utf8(svg).unwrap().to_string()
}

/// Renders a mask of a specified size and returns an RGBA image with _premultiplied_ alpha
pub fn generate_mask(mask: MaskConfig, mask_width: u32, mask_height: u32) -> image::RgbaImage {
    let svg_text = generate_mask_svg(mask, mask_width, mask_height);

    let svg_tree = {
        let mut opt = usvg::Options::default();

        opt.fontdb_mut()
            .load_font_data(include_bytes!("../fonts/Comic Sans MS.ttf").into());

        usvg::Tree::from_str(&svg_text, &opt).unwrap()
    };

    let pixmap_size = IntSize::from_wh(mask_width, mask_height).unwrap();
    let mut pixmap = tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height()).unwrap();
    resvg::render(
        &svg_tree,
        tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );

    image::RgbaImage::from_raw(mask_width, mask_height, pixmap.take()).unwrap()
}

pub trait FromRgba {
    fn from_rgba(rgba: Rgba<u8>) -> Self;
}

impl FromRgba for Rgba<u8> {
    fn from_rgba(rgba: Rgba<u8>) -> Self {
        rgba
    }
}

impl FromRgba for Rgb<u8> {
    fn from_rgba(rgba: Rgba<u8>) -> Self {
        Rgb([rgba[0], rgba[1], rgba[2]])
    }
}

pub fn apply_mask<I, P, T>(mask: MaskConfig, image: &mut I)
where
    I: GenericImage<Pixel = P>,
    P: Pixel<Subpixel = T> + FromRgba,
    T: Primitive,
{
    let mask = generate_mask(mask, image.width(), image.height());

    assert_eq!(image.width(), mask.width());

    for (y, mask_row) in (0..).zip(mask.rows()) {
        for (x, &fg_pix) in (0..).zip(mask_row) {
            // SAFETY: the mask has the same size as the image
            let bg_pix = unsafe { image.unsafe_get_pixel(x, y) };

            let bg_pix = bg_pix.to_rgba();

            // the code here is based on `impl<T: Primitive> Blend for Rgba<T>` from the `image` crate
            let max_t = T::DEFAULT_MAX_VALUE;
            let max_t = max_t.to_f32().unwrap();
            let Rgba([bg_r, bg_g, bg_b, bg_a]) = bg_pix;
            let Rgba([fg_r, fg_g, fg_b, fg_a]) = fg_pix;
            let (bg_r, bg_g, bg_b, bg_a) = (
                bg_r.to_f32().unwrap() / max_t,
                bg_g.to_f32().unwrap() / max_t,
                bg_b.to_f32().unwrap() / max_t,
                bg_a.to_f32().unwrap() / max_t,
            );
            let (fg_r, fg_g, fg_b, fg_a) = (
                fg_r.to_f32().unwrap() / u8::MAX as f32,
                fg_g.to_f32().unwrap() / u8::MAX as f32,
                fg_b.to_f32().unwrap() / u8::MAX as f32,
                fg_a.to_f32().unwrap() / u8::MAX as f32,
            );

            // Work out what the final alpha level will be
            let alpha_final = bg_a + fg_a - bg_a * fg_a;
            // if alpha_final == 0.0 {
            //     return;
            // };

            // We premultiply our channels by their alpha, as this makes it easier to calculate
            let (bg_r_a, bg_g_a, bg_b_a) = (bg_r * bg_a, bg_g * bg_a, bg_b * bg_a);
            // the fg is already premultiplied
            let (fg_r_a, fg_g_a, fg_b_a) = (fg_r, fg_g, fg_b);

            // Standard formula for src-over alpha compositing
            let (out_r_a, out_g_a, out_b_a) = (
                fg_r_a + bg_r_a * (1.0 - fg_a),
                fg_g_a + bg_g_a * (1.0 - fg_a),
                fg_b_a + bg_b_a * (1.0 - fg_a),
            );

            // Unmultiply the channels by our resultant alpha channel
            let (out_r, out_g, out_b) = (
                out_r_a / alpha_final,
                out_g_a / alpha_final,
                out_b_a / alpha_final,
            );
            // get rid of infinities
            let (out_r, out_g, out_b) = (
                if out_r.is_finite() { out_r } else { 0.0 },
                if out_g.is_finite() { out_g } else { 0.0 },
                if out_b.is_finite() { out_b } else { 0.0 },
            );

            // Cast back to our initial type on return
            let im_pix = Rgba::<u8>([
                NumCast::from(max_t * out_r).unwrap(),
                NumCast::from(max_t * out_g).unwrap(),
                NumCast::from(max_t * out_b).unwrap(),
                NumCast::from(max_t * alpha_final).unwrap(),
            ]);

            let im_pix = P::from_rgba(im_pix);

            // SAFETY: the mask has the same size as the image
            unsafe {
                image.unsafe_put_pixel(x, y, im_pix);
            }
        }
    }
}

// const TEST_MASK: Mask = Mask::Stripes {
//     color1: Rgba([255, 0, 0, 32]),
//     color2: Rgba([0, 255, 0, 32]),
//     stripe_count: 10,
// };

#[cfg(test)]
mod tests {
    use super::{apply_mask, generate_mask, generate_mask_svg, Color, MaskConfig, MaskContent};

    fn get_test_mask() -> MaskConfig {
        MaskConfig {
            alpha: 32,
            content: MaskContent::Text {
                text: "ЧУПЛЫГИН УХОДИ".to_string(),
                font: "Comic Sans MS".to_string(),
                color: Color {
                    rgb: [0xff, 0xff, 0xff],
                },
                size_percent: 5.0,
                rotation: 45.0,
                row_slide_percent: 1.0,
                offset_x_percent: -30.0,
                stride_x_percent: 30.0,
                offset_y_percent: -20.0,
                stride_y_percent: 20.0,
            },
        }
    }

    #[test]
    fn svg_smoke() {
        let svg = generate_mask_svg(get_test_mask(), 100, 100);

        eprintln!("{}", svg);
    }

    #[test]
    fn generate_mask_smoke() {
        let mask = generate_mask(get_test_mask(), 720, 1920);

        mask.save("example_results/mask_premultiplied_720x1920.png")
            .unwrap();
    }

    #[test]
    fn apply_mask_smoke() {
        for example_image in std::fs::read_dir("example_images").unwrap() {
            let example_image_entry = example_image.unwrap();
            println!("Applying to {}", example_image_entry.path().display());
            let mut example_image = image::open(example_image_entry.path()).unwrap().to_rgb8();

            apply_mask(get_test_mask(), &mut example_image);

            example_image
                .save(format!(
                    "example_results/{}",
                    example_image_entry.file_name().into_string().unwrap()
                ))
                .unwrap();
        }
    }
}
