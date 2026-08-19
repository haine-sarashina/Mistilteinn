//! Printing: a laid-out page, cut into sheets and written as a PDF.
//!
//! The engine has no second renderer for paper, and writing one would mean a
//! second set of answers to every question layout already answers. So a printed
//! page is the same page: laid out at the paper's width, painted by the same
//! painter, and cut into sheet-sized strips. Each sheet goes into the PDF as an
//! image, which is what makes the file look exactly like what was on screen.

use std::io::Write;

/// A4 at 96 CSS pixels to the inch, which is the scale CSS lengths are in.
pub const A4_WIDTH_PX: u32 = 794;
pub const A4_HEIGHT_PX: u32 = 1123;

/// A4 in PostScript points, the unit a PDF page box is measured in.
const A4_WIDTH_PT: f32 = 595.28;
const A4_HEIGHT_PT: f32 = 841.89;

/// One sheet: RGBA pixels, top-left origin.
pub struct Sheet {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// How many sheets a document of `content_height` pixels needs.
///
/// Always at least one: a blank page still prints as a blank sheet rather than
/// as an empty file.
pub fn sheet_count(content_height: f32, sheet_height: u32) -> usize {
    let sheets = (content_height / sheet_height as f32).ceil();
    (sheets.max(1.0) as usize).min(MAX_SHEETS)
}

/// A ceiling on how much paper one command can produce.
///
/// A page can be arbitrarily tall — an infinite-scroll feed has no end — and
/// turning all of it into bitmaps would exhaust memory before it finished.
pub const MAX_SHEETS: usize = 200;

/// Wrap a set of sheets into a PDF file.
///
/// Each sheet becomes one page holding a single image drawn to fill it. The
/// pixels are stored as `DeviceRGB` and deflated; alpha is composited onto
/// white first, since paper has no transparency.
pub fn sheets_to_pdf(sheets: &[Sheet]) -> Vec<u8> {
    let mut pdf = PdfWriter::new();

    // Object 1 is the catalogue and object 2 the page tree; the pages and their
    // contents follow, and both need to know each other's numbers, so the two
    // roots are reserved first.
    let catalogue = pdf.reserve();
    let page_tree = pdf.reserve();

    let mut page_ids = Vec::new();
    for sheet in sheets {
        let image_id = pdf.add_stream(
            &format!(
                "/Type /XObject /Subtype /Image /Width {} /Height {} \
                 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode",
                sheet.width, sheet.height
            ),
            &deflate(&flatten_onto_white(&sheet.pixels)),
        );

        // The image is drawn into the unit square by default, so the transform
        // is what scales it up to the sheet.
        let content = format!("q {A4_WIDTH_PT} 0 0 {A4_HEIGHT_PT} 0 0 cm /Im0 Do Q\n");
        let content_id = pdf.add_stream("", content.as_bytes());

        let page_id = pdf.add_object(&format!(
            "<< /Type /Page /Parent {page_tree} 0 R \
             /MediaBox [0 0 {A4_WIDTH_PT} {A4_HEIGHT_PT}] \
             /Resources << /XObject << /Im0 {image_id} 0 R >> >> \
             /Contents {content_id} 0 R >>"
        ));
        page_ids.push(page_id);
    }

    let kids = page_ids
        .iter()
        .map(|id| format!("{id} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    pdf.fill_reserved(
        page_tree,
        &format!(
            "<< /Type /Pages /Kids [{kids}] /Count {} >>",
            page_ids.len()
        ),
    );
    pdf.fill_reserved(
        catalogue,
        &format!("<< /Type /Catalog /Pages {page_tree} 0 R >>"),
    );

    pdf.finish(catalogue)
}

/// Drop the alpha channel, compositing onto white.
///
/// The composite bitmap is premultiplied, so a half-covered pixel already
/// carries half its colour; what is missing is the paper showing through.
fn flatten_onto_white(rgba: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for pixel in rgba.chunks_exact(4) {
        let uncovered = 255 - pixel[3] as u16;
        for channel in &pixel[..3] {
            rgb.push((*channel as u16 + uncovered).min(255) as u8);
        }
    }
    rgb
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = encoder.write_all(data);
    encoder.finish().unwrap_or_default()
}

/// Enough of a PDF writer to hold a stack of page images.
///
/// A PDF is a list of numbered objects plus a table saying where each one
/// starts, so writing one is mostly bookkeeping: build the body, remember every
/// offset, and print the table at the end.
struct PdfWriter {
    body: Vec<u8>,
    /// Byte offset of each object, indexed from object 1.
    offsets: Vec<usize>,
    /// Objects whose number was handed out before their content was known.
    reserved: Vec<(usize, String)>,
}

impl PdfWriter {
    fn new() -> Self {
        Self {
            body: b"%PDF-1.4\n".to_vec(),
            offsets: Vec::new(),
            reserved: Vec::new(),
        }
    }

    /// Claim the next object number without writing anything yet.
    fn reserve(&mut self) -> usize {
        self.offsets.push(0);
        self.offsets.len()
    }

    fn fill_reserved(&mut self, id: usize, content: &str) {
        self.reserved.push((id, content.to_string()));
    }

    fn add_object(&mut self, content: &str) -> usize {
        let id = self.reserve();
        self.write_object(id, content.as_bytes());
        id
    }

    fn add_stream(&mut self, dictionary_extras: &str, data: &[u8]) -> usize {
        let id = self.reserve();
        let mut object = Vec::new();
        object.extend_from_slice(
            format!("<< {dictionary_extras} /Length {} >>\nstream\n", data.len()).as_bytes(),
        );
        object.extend_from_slice(data);
        object.extend_from_slice(b"\nendstream");
        self.write_object(id, &object);
        id
    }

    fn write_object(&mut self, id: usize, content: &[u8]) {
        self.offsets[id - 1] = self.body.len();
        self.body
            .extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        self.body.extend_from_slice(content);
        self.body.extend_from_slice(b"\nendobj\n");
    }

    fn finish(mut self, root: usize) -> Vec<u8> {
        let reserved = std::mem::take(&mut self.reserved);
        for (id, content) in reserved {
            self.write_object(id, content.as_bytes());
        }

        let xref_offset = self.body.len();
        let count = self.offsets.len() + 1;
        self.body
            .extend_from_slice(format!("xref\n0 {count}\n0000000000 65535 f \n").as_bytes());
        for offset in &self.offsets {
            self.body
                .extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        self.body.extend_from_slice(
            format!(
                "trailer\n<< /Size {count} /Root {root} 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n"
            )
            .as_bytes(),
        );
        self.body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_sheet() -> Sheet {
        Sheet {
            width: 4,
            height: 4,
            pixels: vec![0u8; 4 * 4 * 4],
        }
    }

    #[test]
    fn a_short_page_still_prints_one_sheet() {
        assert_eq!(sheet_count(0.0, A4_HEIGHT_PX), 1);
        assert_eq!(sheet_count(10.0, A4_HEIGHT_PX), 1);
    }

    #[test]
    fn a_page_is_cut_into_as_many_sheets_as_it_needs() {
        assert_eq!(sheet_count(A4_HEIGHT_PX as f32, A4_HEIGHT_PX), 1);
        assert_eq!(sheet_count(A4_HEIGHT_PX as f32 + 1.0, A4_HEIGHT_PX), 2);
        assert_eq!(sheet_count(A4_HEIGHT_PX as f32 * 3.5, A4_HEIGHT_PX), 4);
    }

    #[test]
    fn an_endless_page_is_capped_rather_than_printed_forever() {
        assert_eq!(sheet_count(f32::MAX, A4_HEIGHT_PX), MAX_SHEETS);
    }

    #[test]
    fn transparent_pixels_come_out_as_white_paper() {
        let flattened = flatten_onto_white(&[0, 0, 0, 0]);
        assert_eq!(flattened, vec![255, 255, 255]);
    }

    #[test]
    fn an_opaque_pixel_keeps_its_colour() {
        let flattened = flatten_onto_white(&[10, 20, 30, 255]);
        assert_eq!(flattened, vec![10, 20, 30]);
    }

    #[test]
    fn a_half_covered_pixel_is_mixed_with_the_paper() {
        // Premultiplied: half-covered black is (0, 0, 0, 128).
        let flattened = flatten_onto_white(&[0, 0, 0, 128]);
        assert_eq!(flattened, vec![127, 127, 127]);
    }

    #[test]
    fn the_file_is_a_pdf_with_the_pages_it_was_given() {
        let pdf = sheets_to_pdf(&[blank_sheet(), blank_sheet()]);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.trim_end().ends_with("%%EOF"));
        assert_eq!(
            text.matches("/Type /Page\n").count() + text.matches("/Type /Page ").count(),
            2
        );
        assert!(text.contains("/Count 2"));
    }

    /// The byte index within `haystack` where `needle` last occurs.
    fn rfind_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .rposition(|window| window == needle)
    }

    #[test]
    fn every_object_offset_points_at_the_object_it_names() {
        // The file holds compressed image data, so the offsets have to be read
        // out of the bytes: a lossy decode to text moves everything after the
        // first byte that is not valid UTF-8.
        let pdf = sheets_to_pdf(&[blank_sheet()]);

        let startxref = rfind_bytes(&pdf, b"startxref").expect("the trailer names the table");
        let tail = String::from_utf8_lossy(&pdf[startxref..]).to_string();
        let xref_at: usize = tail
            .trim_start_matches("startxref")
            .split("%%EOF")
            .next()
            .and_then(|number| number.trim().parse().ok())
            .expect("and gives its offset");
        assert!(pdf[xref_at..].starts_with(b"xref"), "and it is there");

        // "xref", the range header, then the free entry for object 0; the
        // real objects start after those three.
        let table = String::from_utf8_lossy(&pdf[xref_at..]).to_string();
        for (index, line) in table
            .lines()
            .skip(3)
            .take_while(|line| line.ends_with(" n "))
            .enumerate()
        {
            let offset: usize = line[..10].parse().expect("a ten-digit offset");
            let expected = format!("{} 0 obj", index + 1);
            assert!(
                pdf[offset..].starts_with(expected.as_bytes()),
                "object {} should start at {offset}",
                index + 1
            );
        }
    }

    #[test]
    fn a_document_with_no_sheets_is_still_a_readable_file() {
        let pdf = sheets_to_pdf(&[]);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("/Count 0"));
    }

    #[test]
    fn the_image_data_is_compressed() {
        // A blank sheet is highly repetitive, so deflate should shrink it a lot.
        let sheet = Sheet {
            width: 64,
            height: 64,
            pixels: vec![255u8; 64 * 64 * 4],
        };
        let raw = sheet.pixels.len() / 4 * 3;
        let pdf = sheets_to_pdf(&[sheet]);
        assert!(
            pdf.len() < raw / 4,
            "{} bytes for {raw} of pixels",
            pdf.len()
        );
    }
}
