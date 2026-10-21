//! Card titles read with Windows' own text reader (Windows.Media.Ocr): English when that language pack is
//! installed, else the first language of the user's profile (Latin letters read fine either way; the names
//! are matched loosely). Upstream reads them with Tesseract; when neither is there, the pictures decide.

use vorax_scan::pixels::Img;
use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

pub struct Ocr {
    engine: OcrEngine,
    pub language: String,
}

impl Ocr {
    pub fn new() -> windows::core::Result<Ocr> {
        let en = Language::CreateLanguage(&HSTRING::from("en-US"))?;
        let engine = if OcrEngine::IsLanguageSupported(&en)? { OcrEngine::TryCreateFromLanguage(&en)? } else { OcrEngine::TryCreateFromUserProfileLanguages()? };
        let language = engine.RecognizerLanguage()?.LanguageTag()?.to_string();
        Ok(Ocr { engine, language })
    }

    /// The text in a picture (dark text on white), line by line.
    pub fn read(&self, img: &Img) -> windows::core::Result<String> {
        let bmp = SoftwareBitmap::CreateWithAlpha(BitmapPixelFormat::Bgra8, img.width as i32, img.height as i32, BitmapAlphaMode::Premultiplied)?;
        let mut bgra = img.data.clone();
        for p in bgra.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        let writer = DataWriter::new()?;
        writer.WriteBytes(&bgra)?;
        bmp.CopyFromBuffer(&writer.DetachBuffer()?)?;
        let result = self.engine.RecognizeAsync(&bmp)?.get()?;
        let mut lines = Vec::new();
        for line in result.Lines()? {
            lines.push(line.Text()?.to_string());
        }
        Ok(lines.join("\n"))
    }
}
