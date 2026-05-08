//! Parse Sutter / Lexmark DICOM exports and render preview PNGs.
//!
//! Supports DICOM transfer syntaxes that dicom-pixeldata can decode out of the
//! box plus baseline + lossless JPEG via the `jpeg` feature. JPEG-2000 and a
//! handful of exotic syntaxes will fail to decode; in that case the original
//! `.dcm` is still stored and downloadable, just without an inline preview.

use std::io::Cursor;

use dicom_dictionary_std::tags;
use dicom_object::{FileDicomObject, InMemDicomObject};
use dicom_pixeldata::{ConvertOptions, ModalityLutOption, PixelDecoder, VoiLutOption};
use serde::Serialize;
use time::{Date, Month};

pub type DicomFile = FileDicomObject<InMemDicomObject>;

#[derive(Debug, thiserror::Error)]
pub enum DicomError {
    #[error("not a DICOM file (missing DICM magic)")]
    NotDicom,
    #[error("dicom parse: {0}")]
    Parse(String),
    #[error("dicom pixel decode: {0}")]
    Pixel(String),
    #[error("png encode: {0}")]
    Png(String),
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct DicomMetadata {
    pub study_instance_uid: Option<String>,
    pub series_instance_uid: Option<String>,
    pub sop_instance_uid: Option<String>,
    pub sop_class_uid: Option<String>,
    pub modality: Option<String>,
    pub body_part: Option<String>,
    pub view_position: Option<String>,
    pub laterality: Option<String>,
    pub study_description: Option<String>,
    pub series_description: Option<String>,
    pub patient_name: Option<String>,
    pub institution_name: Option<String>,
    pub study_date: Option<Date>,
    pub instance_number: Option<i32>,
}

pub fn is_dicom(bytes: &[u8]) -> bool {
    bytes.len() >= 132 && &bytes[128..132] == b"DICM"
}

pub fn parse(bytes: &[u8]) -> Result<DicomFile, DicomError> {
    if !is_dicom(bytes) {
        return Err(DicomError::NotDicom);
    }
    dicom_object::FileDicomObject::from_reader(Cursor::new(bytes))
        .map_err(|e| DicomError::Parse(e.to_string()))
}

pub fn extract_metadata(obj: &DicomFile) -> DicomMetadata {
    let s = |tag| -> Option<String> {
        obj.element(tag)
            .ok()
            .and_then(|e| e.to_str().ok().map(|c| c.trim().to_string()))
            .filter(|s| !s.is_empty())
    };
    let i = |tag| -> Option<i32> {
        obj.element(tag).ok().and_then(|e| e.to_int::<i32>().ok())
    };
    let study_date = s(tags::STUDY_DATE).and_then(|raw| parse_dicom_date(&raw));

    DicomMetadata {
        study_instance_uid: s(tags::STUDY_INSTANCE_UID),
        series_instance_uid: s(tags::SERIES_INSTANCE_UID),
        sop_instance_uid: s(tags::SOP_INSTANCE_UID),
        sop_class_uid: s(tags::SOP_CLASS_UID),
        modality: s(tags::MODALITY),
        body_part: s(tags::BODY_PART_EXAMINED),
        view_position: s(tags::VIEW_POSITION),
        laterality: s(tags::IMAGE_LATERALITY).or_else(|| s(tags::LATERALITY)),
        study_description: s(tags::STUDY_DESCRIPTION),
        series_description: s(tags::SERIES_DESCRIPTION),
        patient_name: s(tags::PATIENT_NAME),
        institution_name: s(tags::INSTITUTION_NAME),
        study_date,
        instance_number: i(tags::INSTANCE_NUMBER),
    }
}

fn parse_dicom_date(s: &str) -> Option<Date> {
    if s.len() != 8 || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let y: i32 = s[0..4].parse().ok()?;
    let m: u8 = s[4..6].parse().ok()?;
    let d: u8 = s[6..8].parse().ok()?;
    Date::from_calendar_date(y, Month::try_from(m).ok()?, d).ok()
}

pub struct RenderedImages {
    pub png: Vec<u8>,
    pub thumbnail_webp: Vec<u8>,
}

pub fn render_images(obj: &DicomFile) -> Result<RenderedImages, DicomError> {
    let pixel = obj
        .decode_pixel_data()
        .map_err(|e| DicomError::Pixel(e.to_string()))?;

    // Apply Modality LUT (rescale slope/intercept) and VOI LUT (window
    // center / window width) so the rendered image looks the way a
    // radiologist's workstation would display it. Without VOI LUT the
    // 16-bit pixel values get a naive linear-rescale and X-rays come out
    // washed-out or too dark.
    let options = ConvertOptions::new()
        .with_modality_lut(ModalityLutOption::Default)
        .with_voi_lut(VoiLutOption::Default);
    let dyn_img = pixel
        .to_dynamic_image_with_options(0, &options)
        .map_err(|e| DicomError::Pixel(e.to_string()))?;

    let mut png = Vec::new();
    dyn_img
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| DicomError::Png(e.to_string()))?;

    // Thumbnail: drop to 8-bit and resize to fit a 400px box, encode WebP.
    let thumb_8bit = dyn_img.to_rgb8();
    let thumb = image::DynamicImage::ImageRgb8(thumb_8bit).thumbnail(400, 400);
    let mut thumbnail_webp = Vec::new();
    thumb
        .write_to(&mut Cursor::new(&mut thumbnail_webp), image::ImageFormat::WebP)
        .map_err(|e| DicomError::Png(e.to_string()))?;

    Ok(RenderedImages { png, thumbnail_webp })
}

/// True if a DICOM `PatientName` looks like it refers to the same person
/// as the chosen subject. DICOM names are caret-delimited
/// `family^given^middle^prefix^suffix^`. We're forgiving on case,
/// whitespace, and middle/prefix/suffix; both family and given must match.
/// Empty DICOM names return `true` (we can't tell — don't block).
pub fn patient_name_matches(dicom_name: &str, given: &str, family: &str) -> bool {
    let dicom = dicom_name.trim();
    if dicom.is_empty() {
        return true;
    }
    let parts: Vec<String> = dicom
        .split('^')
        .map(|s| s.trim().to_lowercase())
        .collect();
    let dfam = parts.first().cloned().unwrap_or_default();
    let dgiv = parts.get(1).cloned().unwrap_or_default();
    let sfam = family.trim().to_lowercase();
    let sgiv = given.trim().to_lowercase();
    !dfam.is_empty() && !dgiv.is_empty() && dfam == sfam && dgiv == sgiv
}

/// Build a human-readable title from DICOM metadata.
///
/// Examples:
/// - body=WRIST + modality=XR + view=AP → "Wrist X-ray — AP"
/// - body=CHEST + modality=DX (no view) → "Chest X-ray"
/// - series_description="L wrist 2 view" only → "L wrist 2 view"
pub fn derive_title(meta: &DicomMetadata) -> String {
    let body = meta.body_part.as_deref().unwrap_or("").trim();
    let view = meta.view_position.as_deref().unwrap_or("").trim();
    let modality = meta.modality.as_deref().unwrap_or("").trim();
    let series = meta.series_description.as_deref().unwrap_or("").trim();

    let modality_label = modality_label(modality);
    let body_label = if body.is_empty() {
        String::new()
    } else {
        title_case(body)
    };

    let head = match (body_label.is_empty(), modality_label.is_empty()) {
        (false, false) => format!("{body_label} {modality_label}"),
        (false, true) => body_label,
        (true, false) => modality_label.to_string(),
        (true, true) => series.to_string(),
    };

    let tail = if !view.is_empty() {
        format!(" — {view}")
    } else if !series.is_empty()
        && !head.is_empty()
        && !head.to_lowercase().contains(&series.to_lowercase())
    {
        format!(" — {series}")
    } else {
        String::new()
    };

    let combined = format!("{head}{tail}").trim().to_string();
    if combined.is_empty() {
        "DICOM image".to_string()
    } else {
        combined
    }
}

/// Pick the records-table `kind` value. Reports (Structured Reports,
/// Secondary Capture images of typed text, Encapsulated PDF/CDA) are
/// distinguished from primary images so the UI can attach them to their
/// X-ray rather than treating them as another X-ray.
pub fn derive_kind(meta: &DicomMetadata) -> &'static str {
    if is_report(meta) {
        return "report";
    }
    match meta.modality.as_deref().unwrap_or("") {
        "CR" | "DX" | "XR" | "RG" | "RF" | "PX" => "xray",
        "CT" => "ct",
        "MR" => "mri",
        "US" => "ultrasound",
        "MG" => "xray", // mammography — closest existing kind
        "SR" | "DOC" => "report",
        _ => "xray",
    }
}

fn is_report(meta: &DicomMetadata) -> bool {
    if let Some(series) = meta.series_description.as_deref() {
        let s = series.to_lowercase();
        if s.contains("scan") || s.contains("report") || s.contains("document") {
            return true;
        }
    }
    if let Some(sop) = meta.sop_class_uid.as_deref() {
        // Secondary Capture (1.2.840.10008.5.1.4.1.1.7 + .1/.2/.3/.4)
        if sop == "1.2.840.10008.5.1.4.1.1.7"
            || sop.starts_with("1.2.840.10008.5.1.4.1.1.7.")
            // Structured Reports (1.2.840.10008.5.1.4.1.1.88.*)
            || sop.starts_with("1.2.840.10008.5.1.4.1.1.88.")
            // Encapsulated PDF / CDA
            || sop == "1.2.840.10008.5.1.4.1.1.104.1"
            || sop == "1.2.840.10008.5.1.4.1.1.104.2"
        {
            return true;
        }
    }
    false
}

fn modality_label(m: &str) -> &'static str {
    match m {
        "CR" | "DX" | "XR" | "RG" | "RF" | "PX" => "X-ray",
        "CT" => "CT",
        "MR" => "MRI",
        "US" => "Ultrasound",
        "MG" => "Mammogram",
        "" => "",
        _ => "Imaging",
    }
}

fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, word) in s.split_whitespace().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            for c in chars {
                out.extend(c.to_lowercase());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run with:
    ///   DICOM_PROBE_PATH=~/Downloads/xray/DICOM/2 \
    ///     cargo test --release -- dicom_import::tests::probe --nocapture --ignored
    /// Reads a real DICOM file, prints its metadata, and writes a PNG next to it.
    #[test]
    #[ignore]
    fn probe() {
        let path = std::env::var("DICOM_PROBE_PATH").expect("set DICOM_PROBE_PATH");
        let bytes = std::fs::read(&path).expect("read");
        println!("file: {path} ({} bytes)", bytes.len());
        println!("is_dicom: {}", is_dicom(&bytes));
        let obj = parse(&bytes).expect("parse");
        let meta = extract_metadata(&obj);
        println!("metadata: {meta:#?}");
        println!("title: {}", derive_title(&meta));
        println!("kind:  {}", derive_kind(&meta));
        match render_png(&obj) {
            Ok(png) => {
                let stem = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("dicom");
                let out = format!("/tmp/personal-emr-probe-{stem}.png");
                std::fs::write(&out, &png).expect("write png");
                println!("wrote {} ({} bytes)", out, png.len());
            }
            Err(e) => println!("render failed: {e}"),
        }
    }
}
