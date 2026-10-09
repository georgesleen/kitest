//! Saving what the scope shows: the window as a PNG, or the shown traces as CSV.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use eframe::egui::{self, ColorImage};

/// What a save writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// The window as an image.
    Png,
    /// The shown traces over the measurement window, as comma-separated values.
    Csv,
}

impl Format {
    /// The file extension, without the dot.
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Csv => "csv",
        }
    }
}

/// The save dialog: the file to write and its format.
pub struct Dialog {
    path: String,
    format: Format,
}

/// What the dialog's user chose.
pub enum Choice {
    /// Write `format` to `path`.
    Save { path: PathBuf, format: Format },
    /// Close the dialog without saving.
    Cancel,
}

impl Dialog {
    /// A dialog for `format`, defaulting to a file beside `capture` named after it.
    pub fn new(capture: &Path, format: Format) -> Self {
        Self {
            path: capture
                .with_extension(format.extension())
                .display()
                .to_string(),
            format,
        }
    }

    /// Shows the dialog over the window, and returns the choice once one is made.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Choice> {
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("save")).show(ctx, |ui| {
            ui.heading("Save");
            ui.horizontal(|ui| {
                for (format, label) in [(Format::Png, "PNG of the window"), (Format::Csv, "CSV of the shown traces")] {
                    if ui.radio_value(&mut self.format, format, label).changed() {
                        self.path = Path::new(&self.path).with_extension(format.extension()).display().to_string();
                    }
                }
            });
            let field = ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(480.0));
            if self.format == Format::Csv {
                ui.weak("The CSV covers the span between cursors A and B, or the visible window.");
            }
            let entered = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() || entered {
                    choice = Some(Choice::Save {
                        path: PathBuf::from(&self.path),
                        format: self.format,
                    });
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(Choice::Cancel);
        }
        choice
    }
}

/// Why a save failed.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Png(#[from] png::EncodingError),
}

/// Writes `image` to `path` as an 8-bit RGBA PNG.
pub fn write_png(path: &Path, image: &ColorImage) -> Result<(), SaveError> {
    let [width, height] = image.size;
    let mut encoder = png::Encoder::new(
        BufWriter::new(File::create(path)?),
        width as u32,
        height as u32,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let bytes: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|pixel| pixel.to_array())
        .collect();
    encoder.write_header()?.write_image_data(&bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use eframe::egui::{Color32, ColorImage};

    use super::write_png;

    #[test]
    fn a_written_png_reads_back_with_its_size_and_pixels() {
        let pixels = vec![
            Color32::RED,
            Color32::GREEN,
            Color32::BLUE,
            Color32::WHITE,
            Color32::BLACK,
            Color32::YELLOW,
        ];
        let image = ColorImage::new([3, 2], pixels.clone());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("scope.png");
        write_png(&path, &image).unwrap();

        let mut reader = png::Decoder::new(std::io::BufReader::new(
            File::open(&path).unwrap(),
        ))
        .read_info()
        .unwrap();
        let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut bytes).unwrap();
        assert_eq!((info.width, info.height), (3, 2));
        let expected: Vec<u8> =
            pixels.iter().flat_map(|pixel| pixel.to_array()).collect();
        assert_eq!(&bytes[..info.buffer_size()], expected.as_slice());
    }
}
